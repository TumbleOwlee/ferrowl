//! Modbus/TCP TLS configuration (MB-R-104 .. MB-R-112, MB-R-136, MB-R-138/139), plus the
//! client/server TLS glue shared by every TCP-framed transport (`tcp`, and — MB-R-115 —
//! `rtu_over_tcp`/`ascii_over_tcp`).

use serde::{Deserialize, Serialize};

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};
use parking_lot::Mutex;
use rust_modbus::{
    ClientCertPolicy, ClientIdentity, RootStore, ServerCertVerification, TlsClientConfig,
    TlsServerConfig, load_pem_cert_chain, load_pem_private_key,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::TcpError;

/// TLS material for a Modbus/TCP endpoint (MB-R-105): a role-pure `server` policy (consulted
/// only when this endpoint runs as a server) and a role-pure `client` policy (consulted only
/// when this endpoint runs as a client) — both fields are always present on the wire (flattened
/// siblings), even though only one is read at any given call site.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ModbusTlsConfig {
    #[serde(flatten)]
    pub server: ServerTlsPolicy,
    #[serde(flatten)]
    pub client: ClientTlsPolicy,
}

impl Default for ModbusTlsConfig {
    fn default() -> Self {
        ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Unset,
            },
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify { ca_file: None },
            },
        }
    }
}

/// A self-signed certificate/key pair generated once per module instance and reused across
/// bind/connect/reconnect attempts, cleared whenever the resolved source moves away from
/// self-signed so a later reversion regenerates fresh material (MB-R-106/MB-R-138).
pub type SelfSignedCache =
    Arc<Mutex<Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>>>;

/// A fresh, empty cache — call exactly once per module instance (e.g. in `ModbusModule::new`),
/// never per-`reconfigure()` call, so a config edit that keeps the source self-signed reuses the
/// same material instead of regenerating it.
pub fn new_self_signed_cache() -> SelfSignedCache {
    Arc::new(Mutex::new(None))
}

/// Read a PEM file's raw bytes, mapping a missing/unreadable file to the TLS
/// configuration-error tier (MB-R-107/108, edge-cases.md "malformed PEM").
pub(crate) fn read_pem(path: &str) -> Result<Vec<u8>, TcpError> {
    let resolved = ferrowl_util::path::expand(path);
    std::fs::read(&resolved)
        .map_err(|e| TcpError::Configuration(format!("failed to read {path}: {e}")))
}

/// Map any `rust_modbus` failure encountered while assembling TLS material (a bad
/// PEM, an untrusted root, ...) onto the same configuration-error tier.
pub(crate) fn map_tls_err(e: rust_modbus::Error) -> TcpError {
    TcpError::Configuration(e.to_string())
}

/// A client-side socket that is either plain TCP or TLS-terminated TCP (MB-R-104),
/// shared by every TCP-framed client (`tcp`, and — MB-R-115 — `rtu_over_tcp`).
pub(crate) enum ClientStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl AsyncRead for ClientStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ClientStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_flush(cx),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Resolve a self-signed pair via the cache/regenerate rule (MB-R-106/MB-R-138, shared design —
/// `## Shared` in the tls-mtls-role-split plan): reused whenever the resolved source stays
/// self-signed, regenerated the first time or after any transition away from self-signed cleared
/// it. `PrivateKeyDer` is not `Clone` (by design, `rustls-pki-types` 1.15.1); `clone_key()` is
/// its explicit deep-copy escape hatch, used on every cache hit.
fn resolve_self_signed(
    host: &str,
    cache: &SelfSignedCache,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TcpError> {
    let mut guard = cache.lock();
    if let Some((chain, key)) = guard.as_ref() {
        return Ok((chain.clone(), key.clone_key()));
    }
    let (chain, key) = generate_self_signed(host)?;
    *guard = Some((chain.clone(), key.clone_key()));
    Ok((chain, key))
}

/// Build the `rust_modbus` client TLS config from a resolved [`ClientTlsPolicy`]: `NoTls` means
/// this endpoint runs plain TCP entirely (no `TlsClientConfig` built at all — `Ok(None)`).
/// `Tls`/`MutualTls` build server-certificate verification identically (MB-R-109: skip-verify
/// wins over `ca_file`, never combined); `MutualTls` additionally presents a client identity —
/// `Explicit` loads from disk, `SelfSigned` generates/reuses via the cache rule (MB-R-138).
pub(crate) fn build_client_tls_config(
    policy: &ClientTlsPolicy,
    cache: &SelfSignedCache,
) -> Result<Option<TlsClientConfig>, TcpError> {
    let (client_verification, client_identity_source) = match policy {
        ClientTlsPolicy::NoTls => return Ok(None),
        ClientTlsPolicy::Tls {
            client_verification,
        } => (client_verification, None),
        ClientTlsPolicy::MutualTls {
            client_verification,
            client_identity,
        } => (client_verification, Some(client_identity)),
    };
    let server_cert = match client_verification {
        ClientVerification::SkipVerify => ServerCertVerification::DangerousDisableVerification,
        ClientVerification::Verify { ca_file } => {
            let mut roots = RootStore::native();
            if let Some(path) = ca_file {
                roots.add_pem(&read_pem(path)?).map_err(map_tls_err)?;
            }
            ServerCertVerification::Verify(roots)
        }
    };
    let client_identity = match client_identity_source {
        Some(ClientCertSource::Explicit {
            client_cert_file,
            client_key_file,
        }) => Some(ClientIdentity {
            cert_chain: load_pem_cert_chain(&read_pem(client_cert_file)?).map_err(map_tls_err)?,
            key: load_pem_private_key(&read_pem(client_key_file)?).map_err(map_tls_err)?,
        }),
        Some(ClientCertSource::SelfSigned) => {
            let (cert_chain, key) = resolve_self_signed("ferrowl-modbus-client", cache)?;
            Some(ClientIdentity { cert_chain, key })
        }
        None => None,
    };
    Ok(Some(TlsClientConfig {
        server_cert,
        client_identity,
    }))
}

/// Resolve the server's presented certificate per MB-R-106, and whether an
/// ephemeral self-signed certificate was used *without* being explicitly
/// requested (the caller logs that case). `self_signed` wins unconditionally over
/// `cert_file`/`key_file` (edge-cases.md "TLS boundaries") — enforced by
/// `ServerCertSource` at construction (MB-R-107), so the "one set, not the other"
/// case is unrepresentable here and needs no error arm.
pub(crate) fn resolve_server_identity(
    server_cert: &ServerCertSource,
    bind_host: &str,
    cache: &SelfSignedCache,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>, bool), TcpError> {
    match server_cert {
        ServerCertSource::SelfSigned => {
            let (chain, k) = resolve_self_signed(bind_host, cache)?;
            Ok((chain, k, false))
        }
        ServerCertSource::Explicit {
            cert_file,
            key_file,
        } => {
            // Any explicit configuration clears the cache: a later reversion to self-signed
            // must regenerate rather than reuse material from before the explicit interlude.
            *cache.lock() = None;
            let chain =
                rust_modbus::load_pem_cert_chain(&read_pem(cert_file)?).map_err(map_tls_err)?;
            // A PEM document with no certificate blocks at all (garbage input) parses
            // successfully to an empty chain rather than erroring; catch that here so it
            // fails at the same TLS-configuration-error tier as every other malformed-PEM
            // case (edge-cases.md "TLS boundaries"), rather than surfacing later, at TLS
            // listener bind time, as a bare `Error::Server(Error::TlsHandshake)`.
            if chain.is_empty() {
                return Err(TcpError::Configuration(format!(
                    "{cert_file} contains no certificate"
                )));
            }
            let k = rust_modbus::load_pem_private_key(&read_pem(key_file)?).map_err(map_tls_err)?;
            Ok((chain, k, false))
        }
        ServerCertSource::Unset => {
            let (chain, k) = resolve_self_signed(bind_host, cache)?;
            Ok((chain, k, true))
        }
    }
}

/// An ephemeral self-signed certificate/key pair, generated fresh in memory and
/// never written to disk (MB-R-106 fallback). `host` and `"localhost"` (when
/// different) are carried as SAN entries, mirroring
/// `ferrowl-ocpp/src/security.rs::generate_self_signed`.
fn generate_self_signed(
    host: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TcpError> {
    let mut names = vec![host.to_string()];
    if host != "localhost" {
        names.push("localhost".to_string());
    }
    let mut params = rcgen::CertificateParams::new(names)
        .map_err(|e| TcpError::Configuration(format!("self-signed cert generation failed: {e}")))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "ferrowl Modbus");
    let key_pair = rcgen::KeyPair::generate()
        .map_err(|e| TcpError::Configuration(format!("self-signed key generation failed: {e}")))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| TcpError::Configuration(format!("self-signed cert generation failed: {e}")))?;
    let cert_der = cert.der().clone();
    let key_der = PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(|e| TcpError::Configuration(format!("self-signed key encoding failed: {e}")))?;
    Ok((vec![cert_der], key_der))
}

/// Build the full `rust_modbus` server TLS config from a resolved [`ServerTlsPolicy`]: `NoTls`
/// means this endpoint runs plain TCP entirely (`Ok(None)`, caller binds a bare `TcpListener`).
/// `Tls` never requests a client certificate (`ClientCertPolicy::None`) — `MutualTls` is the
/// sole trigger, in either verification mode (MB-R-108). `MutualTls`'s `Verify{ca_files}` trusts
/// a client certificate signed by *any one* of the configured CAs (`RootStore::add_pem` called
/// once per file, accumulating into one trust store — MB-R-108/MB-R-136's "any one is
/// sufficient, not all").
///
/// `MutualTls`'s `SkipVerify` ("require a client cert but skip chain validation") maps to
/// `ClientCertPolicy::AllowAny` — added upstream at
/// <https://github.com/TumbleOwlee/rust-modbus/issues/40> (rev pinned in the workspace
/// `Cargo.toml` includes the fix) specifically to cover this case: a handshake presenting no
/// certificate still fails, exactly like `Require`, but a presented certificate's chain/identity
/// is never checked.
pub(crate) fn build_server_tls_config(
    policy: &ServerTlsPolicy,
    bind_host: &str,
    cache: &SelfSignedCache,
) -> Result<Option<(TlsServerConfig, bool)>, TcpError> {
    let (server_cert, client_verification) = match policy {
        ServerTlsPolicy::NoTls => return Ok(None),
        ServerTlsPolicy::Tls { server_cert } => (server_cert, None),
        ServerTlsPolicy::MutualTls {
            server_cert,
            client_verification,
        } => (server_cert, Some(client_verification)),
    };
    let (cert_chain, key, used_fallback) = resolve_server_identity(server_cert, bind_host, cache)?;
    let client_certs = match client_verification {
        None => ClientCertPolicy::None,
        Some(ClientCertVerification::Verify { ca_files }) => {
            let mut roots = RootStore::empty();
            for ca in ca_files {
                roots.add_pem(&read_pem(ca)?).map_err(map_tls_err)?;
            }
            ClientCertPolicy::Require(roots)
        }
        Some(ClientCertVerification::SkipVerify) => ClientCertPolicy::AllowAny,
    };
    Ok(Some((
        TlsServerConfig {
            cert_chain,
            key,
            client_certs,
        },
        used_fallback,
    )))
}

#[cfg(test)]
mod tests {
    use super::{ModbusTlsConfig, SelfSignedCache, new_self_signed_cache};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn write_pem(label: &str, pem: &str) -> String {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrowl-modbus-client-tls-ut-{}-{label}-{n}.pem",
            std::process::id()
        ));
        std::fs::write(&path, pem).expect("write test pem");
        path.to_string_lossy().into_owned()
    }

    #[test]
    /// NF-R-042 — `read_pem` expands a leading `~` in the cert/key/CA path.
    fn ut_read_pem_expands_tilde() {
        use super::read_pem;
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        let filename = format!("ferrowl-modbus-tls-tilde-{}.pem", std::process::id());
        let path = home.join(&filename);
        std::fs::write(&path, b"PEMDATA").expect("write test pem");

        let result = read_pem(&format!("~/{filename}"));
        let _ = std::fs::remove_file(&path);

        assert_eq!(result.unwrap(), b"PEMDATA".to_vec());
    }

    fn cert_and_key_pem() -> (String, String) {
        let key = rcgen::KeyPair::generate().expect("keypair generation failed");
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .expect("cert params")
            .self_signed(&key)
            .expect("self-signed cert");
        (cert.pem(), key.serialize_pem())
    }

    /// MB-R-105 — `ModbusTlsConfig`'s default resolves both roles to their off/unset `Tls`
    /// state (never `NoTls` — that variant is only produced by the outer `Option` accessor).
    #[test]
    fn ut_modbus_tls_config_defaults() {
        use ferrowl_util::tls::{
            ClientTlsPolicy, ClientVerification, ServerCertSource, ServerTlsPolicy,
        };

        let cfg = ModbusTlsConfig::default();
        assert_eq!(
            cfg.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Unset
            }
        );
        assert_eq!(
            cfg.client,
            ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify { ca_file: None }
            }
        );
    }

    /// MB-R-110 — a client identity is presented only under `MutualTls`; a bare `Tls` policy
    /// never presents one, regardless of what would otherwise be configured.
    #[test]
    fn ut_build_client_tls_config_identity_only_under_mutual_tls() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{ClientCertSource, ClientTlsPolicy, ClientVerification};

        let cache = new_self_signed_cache();
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem("cert", &cert_pem);
        let key_file = write_pem("key", &key_pem);

        let mtls = ClientTlsPolicy::MutualTls {
            client_verification: ClientVerification::Verify { ca_file: None },
            client_identity: ClientCertSource::Explicit {
                client_cert_file: cert_file,
                client_key_file: key_file,
            },
        };
        let built = build_client_tls_config(&mtls, &cache)
            .expect("builds")
            .expect("Some, MutualTls is TLS");
        assert!(built.client_identity.is_some());

        let tls_only = ClientTlsPolicy::Tls {
            client_verification: ClientVerification::Verify { ca_file: None },
        };
        let built = build_client_tls_config(&tls_only, &cache)
            .expect("builds")
            .expect("Some, Tls is TLS");
        assert!(built.client_identity.is_none());
    }

    /// MB-R-104 — `NoTls` builds nothing at all: the caller runs plain TCP.
    #[test]
    fn ut_build_client_tls_config_no_tls_builds_none() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::ClientTlsPolicy;

        let cache = new_self_signed_cache();
        let built = build_client_tls_config(&ClientTlsPolicy::NoTls, &cache).expect("builds");
        assert!(built.is_none());
    }

    /// MB-R-109 — `SkipVerify` disables server certificate verification and ignores `ca_file`,
    /// rather than combining the two.
    #[test]
    fn ut_build_client_tls_config_client_verification_skip_wins() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{ClientTlsPolicy, ClientVerification};
        use rust_modbus::ServerCertVerification;

        let cache = new_self_signed_cache();
        let policy = ClientTlsPolicy::Tls {
            client_verification: ClientVerification::SkipVerify,
        };
        let built = build_client_tls_config(&policy, &cache)
            .expect("builds despite no ca_file")
            .expect("Some");
        assert!(matches!(
            built.server_cert,
            ServerCertVerification::DangerousDisableVerification
        ));
    }

    /// MB-R-138 — a `SelfSigned` client identity is generated once and reused across repeated
    /// calls sharing the same cache (cache hit, not a fresh key pair each time).
    #[test]
    fn ut_build_client_tls_config_self_signed_identity_reuses_cache() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{ClientCertSource, ClientTlsPolicy, ClientVerification};

        let cache: SelfSignedCache = new_self_signed_cache();
        let policy = ClientTlsPolicy::MutualTls {
            client_verification: ClientVerification::Verify { ca_file: None },
            client_identity: ClientCertSource::SelfSigned,
        };
        let first = build_client_tls_config(&policy, &cache)
            .expect("builds")
            .expect("Some")
            .client_identity
            .expect("self-signed identity present");
        let second = build_client_tls_config(&policy, &cache)
            .expect("builds")
            .expect("Some")
            .client_identity
            .expect("self-signed identity present");
        assert_eq!(
            first.cert_chain, second.cert_chain,
            "cache hit reuses the same cert"
        );
    }

    /// MB-R-106 — a `SelfSigned` server_cert never reads cert_file/key_file (there are none in
    /// the variant) and is not the fallback (used_fallback == false: it was explicitly asked
    /// for).
    #[test]
    fn ut_resolve_server_identity_self_signed_variant_never_reads_disk() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cache = new_self_signed_cache();
        let (_chain, _key, used_fallback) =
            resolve_server_identity(&ServerCertSource::SelfSigned, "localhost", &cache)
                .expect("self-signed generation succeeds");
        assert!(!used_fallback);
    }

    /// MB-R-106 — an `Explicit` server_cert loads exactly the named cert/key PEM files.
    #[test]
    fn ut_resolve_server_identity_explicit_variant_loads_files() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cache = new_self_signed_cache();
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem("srv-cert", &cert_pem);
        let key_file = write_pem("srv-key", &key_pem);

        let source = ServerCertSource::Explicit {
            cert_file,
            key_file,
        };
        let (chain, _key, used_fallback) =
            resolve_server_identity(&source, "localhost", &cache).expect("loads explicit files");
        assert_eq!(chain.len(), 1);
        assert!(!used_fallback);
    }

    /// MB-R-106 — an `Unset` server_cert falls back to an ephemeral self-signed certificate and
    /// flags the fallback so the caller can log it.
    #[test]
    fn ut_resolve_server_identity_unset_variant_falls_back_and_flags_it() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cache = new_self_signed_cache();
        let (_chain, _key, used_fallback) =
            resolve_server_identity(&ServerCertSource::Unset, "localhost", &cache)
                .expect("falls back to self-signed");
        assert!(used_fallback);
    }

    /// MB-R-106 (cache reuse) — a `SelfSigned` server_cert reuses the cached pair across repeat
    /// calls sharing the same cache, rather than regenerating a fresh key pair each time.
    #[test]
    fn ut_resolve_server_identity_self_signed_reuses_cached_pair_across_calls() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cache = new_self_signed_cache();
        let (chain1, _key1, _) =
            resolve_server_identity(&ServerCertSource::SelfSigned, "localhost", &cache)
                .expect("builds");
        let (chain2, _key2, _) =
            resolve_server_identity(&ServerCertSource::SelfSigned, "localhost", &cache)
                .expect("builds");
        assert_eq!(chain1, chain2, "cache hit reuses the same certificate");
    }

    /// MB-R-106 (cache regen) — resolving `Explicit` clears the cache, so a later reversion to
    /// `SelfSigned` regenerates fresh material rather than reusing anything from before the
    /// explicit interlude.
    #[test]
    fn ut_resolve_server_identity_explicit_then_self_signed_regenerates() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cache = new_self_signed_cache();
        let (chain1, _key1, _) =
            resolve_server_identity(&ServerCertSource::SelfSigned, "localhost", &cache)
                .expect("builds");

        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem("explicit-cert", &cert_pem);
        let key_file = write_pem("explicit-key", &key_pem);
        let _ = resolve_server_identity(
            &ServerCertSource::Explicit {
                cert_file,
                key_file,
            },
            "localhost",
            &cache,
        )
        .expect("builds");

        let (chain2, _key2, _) =
            resolve_server_identity(&ServerCertSource::SelfSigned, "localhost", &cache)
                .expect("builds");
        assert_ne!(
            chain1, chain2,
            "an explicit interlude must clear the cache, forcing regeneration"
        );
    }

    /// MB-R-104 — `NoTls` builds nothing at all: the caller binds a plain `TcpListener`.
    #[test]
    fn ut_build_server_tls_config_no_tls_builds_none() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::ServerTlsPolicy;

        let cache = new_self_signed_cache();
        let built =
            build_server_tls_config(&ServerTlsPolicy::NoTls, "localhost", &cache).expect("builds");
        assert!(built.is_none());
    }

    /// MB-R-108 — a bare `Tls` policy never requests a client certificate at all
    /// (`ClientCertPolicy::None`), regardless of how verification would otherwise be configured.
    #[test]
    fn ut_build_server_tls_config_tls_level_never_requests_client_cert() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::{ServerCertSource, ServerTlsPolicy};
        use rust_modbus::ClientCertPolicy;

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::SelfSigned,
        };
        let (built, _fallback) = build_server_tls_config(&policy, "localhost", &cache)
            .expect("builds")
            .expect("Some");
        assert!(matches!(built.client_certs, ClientCertPolicy::None));
    }

    /// MB-R-108/MB-R-136 — `MutualTls`'s `Verify{ca_files}` trusts a client certificate signed
    /// by any *one* of several configured CAs, not all of them: a chain signed only by the
    /// second CA still resolves (config-resolution level; the actual handshake accept is proven
    /// end-to-end by the loopback integration test).
    #[test]
    fn ut_build_server_tls_config_multi_ca_accumulates_into_one_root_store() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::{ClientCertVerification, ServerCertSource, ServerTlsPolicy};
        use rust_modbus::ClientCertPolicy;

        let cache = new_self_signed_cache();
        let (ca1_pem, _) = cert_and_key_pem();
        let (ca2_pem, _) = cert_and_key_pem();
        let ca1 = write_pem("ca1", &ca1_pem);
        let ca2 = write_pem("ca2", &ca2_pem);

        let policy = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::SelfSigned,
            client_verification: ClientCertVerification::Verify {
                ca_files: vec![ca1, ca2],
            },
        };
        let (built, _fallback) = build_server_tls_config(&policy, "localhost", &cache)
            .expect("builds with both CAs accumulated")
            .expect("Some");
        assert!(matches!(built.client_certs, ClientCertPolicy::Require(_)));
    }

    /// MB-R-108 — server-role `SkipVerify` maps to `ClientCertPolicy::AllowAny`: a client
    /// certificate is still required (unlike `Tls`'s `ClientCertPolicy::None`), but no chain/
    /// identity validation is performed against any root store. The actual handshake behavior
    /// this produces (a presented-but-untrusted cert is accepted, no cert at all is rejected) is
    /// proven end-to-end by the loopback integration test in `tests/tcp_tls_server.rs`.
    #[test]
    fn ut_build_server_tls_config_skip_verify_maps_to_allow_any() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::{ClientCertVerification, ServerCertSource, ServerTlsPolicy};
        use rust_modbus::ClientCertPolicy;

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::SelfSigned,
            client_verification: ClientCertVerification::SkipVerify,
        };
        let (built, _fallback) = build_server_tls_config(&policy, "localhost", &cache)
            .expect("builds")
            .expect("Some");
        assert!(matches!(built.client_certs, ClientCertPolicy::AllowAny));
    }
}
