//! Modbus/TCP TLS configuration (MB-R-104 .. MB-R-112, MB-R-136, MB-R-138/139), plus the
//! client/server TLS glue shared by every TCP-framed transport (`tcp`, and — MB-R-115 —
//! `rtu_over_tcp`/`ascii_over_tcp`).

use serde::{Deserialize, Serialize};

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy, ServerTlsPolicy};
use parking_lot::Mutex;
use rust_modbus::{
    ClientCertPolicy, ClientIdentity, RootStore, ServerCertVerification, TlsClientConfig,
    TlsServerConfig, load_pem_cert_chain, load_pem_private_key,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{SelfSignedStep, TcpError, TlsError};

/// TLS material for a Modbus/TCP endpoint (MB-R-104): a role-pure `server` policy (consulted
/// only when this endpoint runs as a server) and a role-pure `client` policy (consulted only
/// when this endpoint runs as a client), both always present as a two-role container since a
/// device config records no role.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModbusTlsConfig {
    pub server: ServerTlsPolicy,
    pub client: ClientTlsPolicy,
}

impl ModbusTlsConfig {
    /// MB-R-104 — an absent `tls` block, an empty one, and one whose two policies are both
    /// `None` denote the same state; this is what `#[serde(skip_serializing_if)]` checks so a
    /// saved file stays free of two dead tables.
    pub fn is_none(&self) -> bool {
        matches!(self.server, ServerTlsPolicy::None {})
            && matches!(self.client, ClientTlsPolicy::None {})
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
    std::fs::read(&resolved).map_err(|e| {
        TlsError::Io {
            path: path.to_string(),
            source: e,
        }
        .into()
    })
}

/// Map any `rust_modbus` failure encountered while assembling TLS material for the named
/// path onto the typed PEM-parse case (MB-R-251).
fn pem_err(path: &str) -> impl Fn(rust_modbus::Error) -> TcpError + '_ {
    move |e| {
        TlsError::Pem {
            path: path.to_string(),
            source: e,
        }
        .into()
    }
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

/// Resolve a self-signed pair via the cache/regenerate rule (MB-R-106/MB-R-138): reused whenever
/// the resolved source stays self-signed, regenerated the first time or after any transition
/// away from self-signed cleared it. `PrivateKeyDer` is not `Clone` (by design, `rustls-pki-types`
/// 1.15.1); `clone_key()` is its explicit deep-copy escape hatch, used on every cache hit.
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

/// Build the `rust_modbus` client TLS config from a resolved [`ClientTlsPolicy`]: `None {}` means
/// this endpoint runs plain TCP entirely (no `TlsClientConfig` built at all — `Ok(None)`).
/// `Tls`/`Mutual` build server-certificate verification identically per the `CertVerification`
/// mapping (MB-R-109); `Mutual` additionally presents a client identity — `Files` loads from
/// disk, `SelfSigned` generates/reuses via the cache rule (MB-R-138); `Ephemeral` is rejected by
/// `validate()` before either builder runs (MB-R-110).
pub(crate) fn build_client_tls_config(
    policy: &ClientTlsPolicy,
    cache: &SelfSignedCache,
) -> Result<Option<TlsClientConfig>, TcpError> {
    policy
        .validate()
        .map_err(|e| TcpError::Tls(TlsError::from(e)))?;
    let (verification, identity_source) = match policy {
        ClientTlsPolicy::None {} => return Ok(None),
        ClientTlsPolicy::Tls { verification } => (verification, None),
        ClientTlsPolicy::Mutual {
            verification,
            identity,
        } => (verification, Some(identity)),
    };
    let server_cert = match verification {
        CertVerification::Skip {} => ServerCertVerification::DangerousDisableVerification,
        CertVerification::RootStore { extra_ca_files } => {
            let mut roots = RootStore::native();
            for path in extra_ca_files {
                roots.add_pem(&read_pem(path)?).map_err(pem_err(path))?;
            }
            ServerCertVerification::Verify(roots)
        }
        CertVerification::CaFiles { ca_files } => {
            let mut roots = RootStore::empty();
            for path in ca_files {
                roots.add_pem(&read_pem(path)?).map_err(pem_err(path))?;
            }
            ServerCertVerification::Verify(roots)
        }
    };
    let client_identity = match identity_source {
        Some(CertSource::Files {
            cert_file,
            key_file,
        }) => Some(ClientIdentity {
            cert_chain: load_pem_cert_chain(&read_pem(cert_file)?).map_err(pem_err(cert_file))?,
            key: load_pem_private_key(&read_pem(key_file)?).map_err(pem_err(key_file))?,
        }),
        Some(CertSource::SelfSigned {}) => {
            let (cert_chain, key) = resolve_self_signed("ferrowl-modbus-client", cache)?;
            Some(ClientIdentity { cert_chain, key })
        }
        Some(CertSource::Ephemeral {}) => {
            return Err(TlsError::EphemeralClientIdentity.into());
        }
        None => None,
    };
    Ok(Some(TlsClientConfig {
        server_cert,
        client_identity,
    }))
}

/// Resolve the server's presented certificate per MB-R-106, and whether an ephemeral
/// self-signed certificate was used *without* being explicitly requested (the caller logs that
/// case).
pub(crate) fn resolve_server_identity(
    identity: &CertSource,
    bind_host: &str,
    cache: &SelfSignedCache,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>, bool), TcpError> {
    match identity {
        CertSource::SelfSigned {} => {
            let (chain, k) = resolve_self_signed(bind_host, cache)?;
            Ok((chain, k, false))
        }
        CertSource::Files {
            cert_file,
            key_file,
        } => {
            // Any explicit configuration clears the cache: a later reversion to self-signed
            // must regenerate rather than reuse material from before the explicit interlude.
            *cache.lock() = None;
            let chain = rust_modbus::load_pem_cert_chain(&read_pem(cert_file)?)
                .map_err(pem_err(cert_file))?;
            // A PEM document with no certificate blocks at all (garbage input) parses
            // successfully to an empty chain rather than erroring; catch that here so it
            // fails at the same TLS-configuration-error tier as every other malformed-PEM
            // case (edge-cases.md "TLS boundaries"), rather than surfacing later, at TLS
            // listener bind time, as a bare `Error::Server(Error::TlsHandshake)`.
            if chain.is_empty() {
                return Err(TlsError::NoCertificates {
                    path: cert_file.clone(),
                }
                .into());
            }
            let k = rust_modbus::load_pem_private_key(&read_pem(key_file)?)
                .map_err(pem_err(key_file))?;
            Ok((chain, k, false))
        }
        CertSource::Ephemeral {} => {
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
    let mut params = rcgen::CertificateParams::new(names).map_err(|e| TlsError::SelfSigned {
        step: SelfSignedStep::CertGeneration,
        detail: e.to_string(),
    })?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "ferrowl Modbus");
    let key_pair = rcgen::KeyPair::generate().map_err(|e| TlsError::SelfSigned {
        step: SelfSignedStep::KeyGeneration,
        detail: e.to_string(),
    })?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| TlsError::SelfSigned {
            step: SelfSignedStep::CertGeneration,
            detail: e.to_string(),
        })?;
    let cert_der = cert.der().clone();
    let key_der =
        PrivateKeyDer::try_from(key_pair.serialize_der()).map_err(|e| TlsError::SelfSigned {
            step: SelfSignedStep::KeyEncoding,
            detail: e.to_string(),
        })?;
    Ok((vec![cert_der], key_der))
}

/// Build the full `rust_modbus` server TLS config from a resolved [`ServerTlsPolicy`]: `None {}`
/// means this endpoint runs plain TCP entirely (`Ok(None)`, caller binds a bare `TcpListener`).
/// `Tls` never requests a client certificate (`ClientCertPolicy::None`) — `Mutual` is the sole
/// trigger, in either verification mode (MB-R-108). `Mutual`'s `CaFiles{ca_files}` trusts a
/// client certificate signed by *any one* of the configured CAs (`RootStore::add_pem` called once
/// per file, accumulating into one trust store — MB-R-108/MB-R-136's "any one is sufficient, not
/// all"). `RootStore` verification is rejected by `validate()` before either builder runs
/// (MB-R-108).
///
/// `Mutual`'s `Skip {}` ("require a client cert but skip chain validation") maps to
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
    policy
        .validate()
        .map_err(|e| TcpError::Tls(TlsError::from(e)))?;
    let (identity, verification) = match policy {
        ServerTlsPolicy::None {} => return Ok(None),
        ServerTlsPolicy::Tls { identity } => (identity, None),
        ServerTlsPolicy::Mutual {
            identity,
            verification,
        } => (identity, Some(verification)),
    };
    let (cert_chain, key, used_fallback) = resolve_server_identity(identity, bind_host, cache)?;
    let client_certs = match verification {
        None => ClientCertPolicy::None,
        Some(CertVerification::CaFiles { ca_files }) => {
            let mut roots = RootStore::empty();
            for ca in ca_files {
                roots.add_pem(&read_pem(ca)?).map_err(pem_err(ca))?;
            }
            ClientCertPolicy::Require(roots)
        }
        Some(CertVerification::Skip {}) => ClientCertPolicy::AllowAny,
        Some(CertVerification::RootStore { .. }) => {
            return Err(TlsError::RootStoreOnServer.into());
        }
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
    use ferrowl_test_support::{TempDirGuard, reserve_temp_dir};

    fn write_pem(dir: &TempDirGuard, label: &str, pem: &str) -> String {
        let path = dir.join(format!("{label}.pem"));
        std::fs::write(&path, pem).expect("write test pem");
        path.to_string_lossy().into_owned()
    }

    #[test]
    /// NF-R-054 — `read_pem` expands a leading `~` in the cert/key/CA path.
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

    /// MB-R-162 — `ModbusTlsConfig`'s default is both policies `None`, and `is_none()` holds for
    /// it.
    #[test]
    fn ut_modbus_tls_config_defaults_to_both_none() {
        let cfg = ModbusTlsConfig::default();
        assert!(cfg.is_none());
    }

    /// MB-R-110 — a client identity is presented only under `Mutual`; a bare `Tls` policy never
    /// presents one, regardless of what would otherwise be configured.
    #[test]
    fn ut_build_client_tls_config_identity_only_under_mutual() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy};
        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");

        let cache = new_self_signed_cache();
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem(&dir, "cert", &cert_pem);
        let key_file = write_pem(&dir, "key", &key_pem);

        let mtls = ClientTlsPolicy::Mutual {
            verification: CertVerification::RootStore {
                extra_ca_files: vec![],
            },
            identity: CertSource::Files {
                cert_file,
                key_file,
            },
        };
        let built = build_client_tls_config(&mtls, &cache)
            .expect("builds")
            .expect("Some, Mutual is TLS");
        assert!(built.client_identity.is_some());

        let tls_only = ClientTlsPolicy::Tls {
            verification: CertVerification::RootStore {
                extra_ca_files: vec![],
            },
        };
        let built = build_client_tls_config(&tls_only, &cache)
            .expect("builds")
            .expect("Some, Tls is TLS");
        assert!(built.client_identity.is_none());
    }

    /// MB-R-175 — `Mutual` with `CertSource::Files` presents a non-empty chain as the client
    /// identity.
    #[test]
    fn ut_build_client_tls_config_mutual_files_presents_pair() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy};
        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");

        let cache = new_self_signed_cache();
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem(&dir, "cert", &cert_pem);
        let key_file = write_pem(&dir, "key", &key_pem);
        let policy = ClientTlsPolicy::Mutual {
            verification: CertVerification::Skip {},
            identity: CertSource::Files {
                cert_file,
                key_file,
            },
        };
        let built = build_client_tls_config(&policy, &cache)
            .expect("builds")
            .expect("Some");
        let identity = built.client_identity.expect("Some, Mutual presents one");
        assert!(!identity.cert_chain.is_empty());
    }

    /// MB-R-162 — `None {}` builds nothing at all: the caller runs plain TCP.
    #[test]
    fn ut_build_client_tls_config_none_builds_none() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::ClientTlsPolicy;

        let cache = new_self_signed_cache();
        let built = build_client_tls_config(&ClientTlsPolicy::None {}, &cache).expect("builds");
        assert!(built.is_none());
    }

    /// MB-R-109 — `Skip` disables server certificate verification entirely.
    #[test]
    fn ut_build_client_tls_config_verification_skip() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{CertVerification, ClientTlsPolicy};
        use rust_modbus::ServerCertVerification;

        let cache = new_self_signed_cache();
        let policy = ClientTlsPolicy::Tls {
            verification: CertVerification::Skip {},
        };
        let built = build_client_tls_config(&policy, &cache)
            .expect("builds despite no ca_files")
            .expect("Some");
        assert!(matches!(
            built.server_cert,
            ServerCertVerification::DangerousDisableVerification
        ));
    }

    /// MB-R-138 — a `SelfSigned` client identity is generated once and reused across repeated
    /// calls sharing the same cache (cache hit, not a fresh key pair each time).
    #[test]
    fn ut_build_client_tls_config_self_signed_identity_uses_cache() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy};

        let cache: SelfSignedCache = new_self_signed_cache();
        let policy = ClientTlsPolicy::Mutual {
            verification: CertVerification::RootStore {
                extra_ca_files: vec![],
            },
            identity: CertSource::SelfSigned {},
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

    /// MB-R-106 — a `SelfSigned` identity never reads cert_file/key_file (there are none in the
    /// variant) and is not the fallback (used_fallback == false: it was explicitly asked for).
    #[test]
    fn ut_resolve_server_identity_self_signed() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::CertSource;

        let cache = new_self_signed_cache();
        let (_chain, _key, used_fallback) =
            resolve_server_identity(&CertSource::SelfSigned {}, "localhost", &cache)
                .expect("self-signed generation succeeds");
        assert!(!used_fallback);
    }

    /// MB-R-106 — a `Files` identity loads exactly the named cert/key PEM files.
    #[test]
    fn ut_resolve_server_identity_files() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::CertSource;
        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");

        let cache = new_self_signed_cache();
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem(&dir, "srv-cert", &cert_pem);
        let key_file = write_pem(&dir, "srv-key", &key_pem);

        let source = CertSource::Files {
            cert_file,
            key_file,
        };
        let (chain, _key, used_fallback) =
            resolve_server_identity(&source, "localhost", &cache).expect("loads explicit files");
        assert_eq!(chain.len(), 1);
        assert!(!used_fallback);
    }

    /// MB-R-106 — an `Ephemeral` identity falls back to an ephemeral self-signed certificate and
    /// flags the fallback so the caller can log it.
    #[test]
    fn ut_resolve_server_identity_ephemeral() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::CertSource;

        let cache = new_self_signed_cache();
        let (_chain, _key, used_fallback) =
            resolve_server_identity(&CertSource::Ephemeral {}, "localhost", &cache)
                .expect("falls back to self-signed");
        assert!(used_fallback);
    }

    /// MB-R-169 (cache reuse) — a `SelfSigned` identity reuses the cached pair across repeat
    /// calls sharing the same cache, rather than regenerating a fresh key pair each time.
    #[test]
    fn ut_resolve_server_identity_self_signed_reuses_cached_pair_across_calls() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::CertSource;

        let cache = new_self_signed_cache();
        let (chain1, _key1, _) =
            resolve_server_identity(&CertSource::SelfSigned {}, "localhost", &cache)
                .expect("builds");
        let (chain2, _key2, _) =
            resolve_server_identity(&CertSource::SelfSigned {}, "localhost", &cache)
                .expect("builds");
        assert_eq!(chain1, chain2, "cache hit reuses the same certificate");
    }

    /// MB-R-170 (cache regen) — resolving `Files` clears the cache, so a later reversion to
    /// `SelfSigned` regenerates fresh material rather than reusing anything from before the
    /// explicit interlude.
    #[test]
    fn ut_resolve_server_identity_files_then_self_signed_regenerates() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::CertSource;
        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");

        let cache = new_self_signed_cache();
        let (chain1, _key1, _) =
            resolve_server_identity(&CertSource::SelfSigned {}, "localhost", &cache)
                .expect("builds");

        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem(&dir, "explicit-cert", &cert_pem);
        let key_file = write_pem(&dir, "explicit-key", &key_pem);
        let _ = resolve_server_identity(
            &CertSource::Files {
                cert_file,
                key_file,
            },
            "localhost",
            &cache,
        )
        .expect("builds");

        let (chain2, _key2, _) =
            resolve_server_identity(&CertSource::SelfSigned {}, "localhost", &cache)
                .expect("builds");
        assert_ne!(
            chain1, chain2,
            "a Files interlude must clear the cache, forcing regeneration"
        );
    }

    /// MB-R-162 — `None {}` builds nothing at all: the caller binds a plain `TcpListener`.
    #[test]
    fn ut_build_server_tls_config_none_builds_none() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::ServerTlsPolicy;

        let cache = new_self_signed_cache();
        let built = build_server_tls_config(&ServerTlsPolicy::None {}, "localhost", &cache)
            .expect("builds");
        assert!(built.is_none());
    }

    /// MB-R-174 — a bare `Tls` policy never requests a client certificate at all
    /// (`ClientCertPolicy::None`), regardless of how verification would otherwise be configured.
    #[test]
    fn ut_build_server_tls_config_tls_never_requests_client_cert() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::{CertSource, ServerTlsPolicy};
        use rust_modbus::ClientCertPolicy;

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            identity: CertSource::SelfSigned {},
        };
        let (built, _fallback) = build_server_tls_config(&policy, "localhost", &cache)
            .expect("builds")
            .expect("Some");
        assert!(matches!(built.client_certs, ClientCertPolicy::None));
    }

    /// MB-R-108, MB-R-136 — `Mutual`'s `CaFiles{ca_files}` trusts a client certificate signed by
    /// any *one* of several configured CAs, not all of them: a chain signed only by the second CA
    /// still resolves (config-resolution level; the actual handshake accept is proven end-to-end
    /// by the loopback integration test).
    #[test]
    fn ut_build_server_tls_config_multi_ca_accumulates_into_one_root_store() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::{CertSource, CertVerification, ServerTlsPolicy};
        use rust_modbus::ClientCertPolicy;
        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");

        let cache = new_self_signed_cache();
        let (ca1_pem, _) = cert_and_key_pem();
        let (ca2_pem, _) = cert_and_key_pem();
        let ca1 = write_pem(&dir, "ca1", &ca1_pem);
        let ca2 = write_pem(&dir, "ca2", &ca2_pem);

        let policy = ServerTlsPolicy::Mutual {
            identity: CertSource::SelfSigned {},
            verification: CertVerification::CaFiles {
                ca_files: vec![ca1, ca2],
            },
        };
        let (built, _fallback) = build_server_tls_config(&policy, "localhost", &cache)
            .expect("builds with both CAs accumulated")
            .expect("Some");
        assert!(matches!(built.client_certs, ClientCertPolicy::Require(_)));
    }

    /// MB-R-173 — server-role `Skip` maps to `ClientCertPolicy::AllowAny`: a client certificate
    /// is still required (unlike `Tls`'s `ClientCertPolicy::None`), but no chain/identity
    /// validation is performed against any root store. The actual handshake behavior this
    /// produces (a presented-but-untrusted cert is accepted, no cert at all is rejected) is
    /// proven end-to-end by the loopback integration test in `tests/tcp_tls_server.rs`.
    #[test]
    fn ut_build_server_tls_config_skip_verify_maps_to_allow_any() {
        use super::build_server_tls_config;
        use ferrowl_util::tls::{CertSource, CertVerification, ServerTlsPolicy};
        use rust_modbus::ClientCertPolicy;

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Mutual {
            identity: CertSource::SelfSigned {},
            verification: CertVerification::Skip {},
        };
        let (built, _fallback) = build_server_tls_config(&policy, "localhost", &cache)
            .expect("builds")
            .expect("Some");
        assert!(matches!(built.client_certs, ClientCertPolicy::AllowAny));
    }

    /// MB-R-167, MB-R-172, MB-R-251 — `RootStore` verification is rejected on a server before either
    /// builder runs, as the typed `TlsError::RootStoreOnServer` case.
    #[test]
    fn ut_server_policy_rejects_root_store_verification() {
        use super::build_server_tls_config;
        use crate::{TcpError, TlsError};
        use ferrowl_util::tls::{CertSource, CertVerification, ServerTlsPolicy};

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Mutual {
            identity: CertSource::SelfSigned {},
            verification: CertVerification::RootStore {
                extra_ca_files: vec![],
            },
        };
        let result = build_server_tls_config(&policy, "localhost", &cache);
        assert!(matches!(
            result,
            Err(TcpError::Tls(TlsError::RootStoreOnServer))
        ));
    }

    /// MB-R-251, MB-R-167 — a client `Mutual` with an empty `ca_files` list is rejected as the
    /// typed `TlsError::EmptyCaFiles` case.
    #[test]
    fn ut_client_policy_rejects_empty_ca_files() {
        use super::build_client_tls_config;
        use crate::{TcpError, TlsError};
        use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy};

        let cache = new_self_signed_cache();
        let (cert_pem, key_pem) = cert_and_key_pem();
        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");
        let cert_file = write_pem(&dir, "cert", &cert_pem);
        let key_file = write_pem(&dir, "key", &key_pem);
        let policy = ClientTlsPolicy::Mutual {
            verification: CertVerification::CaFiles { ca_files: vec![] },
            identity: CertSource::Files {
                cert_file,
                key_file,
            },
        };
        let result = build_client_tls_config(&policy, &cache);
        assert!(matches!(result, Err(TcpError::Tls(TlsError::EmptyCaFiles))));
    }

    /// MB-R-251, MB-R-167 — a client `Mutual` with `identity: CertSource::Ephemeral` is rejected
    /// as the typed `TlsError::EphemeralClientIdentity` case.
    #[test]
    fn ut_client_policy_rejects_ephemeral_identity() {
        use super::build_client_tls_config;
        use crate::{TcpError, TlsError};
        use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy};

        let cache = new_self_signed_cache();
        let policy = ClientTlsPolicy::Mutual {
            verification: CertVerification::RootStore {
                extra_ca_files: vec![],
            },
            identity: CertSource::Ephemeral {},
        };
        let result = build_client_tls_config(&policy, &cache);
        assert!(matches!(
            result,
            Err(TcpError::Tls(TlsError::EphemeralClientIdentity))
        ));
    }

    /// MB-R-251, MB-E-063 — an unreadable path through `read_pem` yields the typed `Io` case,
    /// with `path` the configured (pre-expansion) string.
    #[test]
    fn ut_read_pem_unreadable_yields_tls_io() {
        use super::read_pem;
        use crate::{TcpError, TlsError};

        let path = "/nonexistent/ferrowl-modbus-tls-test.pem";
        let result = read_pem(path);
        match result {
            Err(TcpError::Tls(TlsError::Io { path: p, .. })) => assert_eq!(p, path),
            other => panic!("expected TcpError::Tls(TlsError::Io), got {other:?}"),
        }
    }

    /// MB-R-251, MB-E-063 — a readable file holding a corrupt PEM block yields the typed `Pem`
    /// case.
    #[test]
    fn ut_build_server_tls_config_corrupt_pem_yields_tls_pem() {
        use super::build_server_tls_config;
        use crate::{TcpError, TlsError};
        use ferrowl_util::tls::{CertSource, ServerTlsPolicy};

        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");
        let cert_file = dir.join("corrupt-cert.pem");
        std::fs::write(
            &cert_file,
            "-----BEGIN CERTIFICATE-----\nnot base64 at all !!!\n-----END CERTIFICATE-----\n",
        )
        .expect("write corrupt pem");
        let (_cert_pem, key_pem) = cert_and_key_pem();
        let key_file = write_pem(&dir, "key", &key_pem);

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            identity: CertSource::Files {
                cert_file: cert_file.to_string_lossy().into_owned(),
                key_file,
            },
        };
        let expected_path = cert_file.to_string_lossy().into_owned();
        let result = build_server_tls_config(&policy, "localhost", &cache);
        match result {
            Err(TcpError::Tls(TlsError::Pem { path, .. })) => assert_eq!(path, expected_path),
            other => panic!("expected TcpError::Tls(TlsError::Pem), got {other:?}"),
        }
    }

    /// MB-R-251, MB-E-063 — a well-formed but certificate-free cert file yields the typed
    /// `NoCertificates` case.
    #[test]
    fn ut_build_server_tls_config_no_certificates_yields_tls_no_certificates() {
        use super::build_server_tls_config;
        use crate::{TcpError, TlsError};
        use ferrowl_util::tls::{CertSource, ServerTlsPolicy};

        let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls");
        let cert_file = write_pem(&dir, "empty-cert", "not a pem file at all");
        let (_cert_pem, key_pem) = cert_and_key_pem();
        let key_file = write_pem(&dir, "key", &key_pem);

        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            identity: CertSource::Files {
                cert_file: cert_file.clone(),
                key_file,
            },
        };
        let result = build_server_tls_config(&policy, "localhost", &cache);
        match result {
            Err(TcpError::Tls(TlsError::NoCertificates { path })) => {
                assert_eq!(path, cert_file)
            }
            other => panic!("expected TlsError::NoCertificates, got {other:?}"),
        }
    }
}
