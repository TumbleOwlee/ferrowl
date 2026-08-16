//! Modbus/TCP TLS configuration (MB-R-104 .. MB-R-112), plus the client/server TLS glue
//! shared by every TCP-framed transport (`tcp`, and — MB-R-115 — `rtu_over_tcp`).

use serde::{Deserialize, Serialize};

use std::pin::Pin;
use std::task::{Context, Poll};

use rust_modbus::{
    ClientCertPolicy, ClientIdentity, RootStore, ServerCertVerification, TlsClientConfig,
    TlsServerConfig, load_pem_cert_chain, load_pem_private_key,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::TcpError;

/// TLS material for a Modbus/TCP endpoint, client or server (MB-R-105).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ModbusTlsConfig {
    #[serde(flatten)]
    pub client_verification: ferrowl_util::tls::ClientVerification,
    #[serde(flatten)]
    pub server_cert: ferrowl_util::tls::ServerCertSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ca_file: Option<String>,
    #[serde(default)]
    pub require_client_cert: bool,
}

/// Read a PEM file's raw bytes, mapping a missing/unreadable file to the TLS
/// configuration-error tier (MB-R-107/108, edge-cases.md "malformed PEM").
pub(crate) fn read_pem(path: &str) -> Result<Vec<u8>, TcpError> {
    std::fs::read(path).map_err(|e| TcpError::Configuration(format!("failed to read {path}: {e}")))
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

/// Build the `rust_modbus` client TLS config from ours: native roots plus `ca_file`
/// if set, unless `insecure_skip_verify` is set (in which case `ca_file` is ignored
/// and no server certificate is checked at all) — MB-R-109. A client identity is
/// presented only when both `client_cert_file` and `client_key_file` are set;
/// either alone presents nothing — MB-R-110.
pub(crate) fn build_client_tls_config(cfg: &ModbusTlsConfig) -> Result<TlsClientConfig, TcpError> {
    let server_cert = match &cfg.client_verification {
        ferrowl_util::tls::ClientVerification::SkipVerify => {
            ServerCertVerification::DangerousDisableVerification
        }
        ferrowl_util::tls::ClientVerification::Verify { ca_file } => {
            let mut roots = RootStore::native();
            if let Some(path) = ca_file {
                roots.add_pem(&read_pem(path)?).map_err(map_tls_err)?;
            }
            ServerCertVerification::Verify(roots)
        }
    };
    let client_identity = match (&cfg.client_cert_file, &cfg.client_key_file) {
        (Some(cert), Some(key)) => Some(ClientIdentity {
            cert_chain: load_pem_cert_chain(&read_pem(cert)?).map_err(map_tls_err)?,
            key: load_pem_private_key(&read_pem(key)?).map_err(map_tls_err)?,
        }),
        _ => None,
    };
    Ok(TlsClientConfig {
        server_cert,
        client_identity,
    })
}

/// Resolve the server's presented certificate per MB-R-106, and whether an
/// ephemeral self-signed certificate was used *without* being explicitly
/// requested (the caller logs that case). `self_signed` wins unconditionally over
/// `cert_file`/`key_file` (edge-cases.md "TLS boundaries") — enforced by
/// `ServerCertSource` at construction (MB-R-107), so the "one set, not the other"
/// case is unrepresentable here and needs no error arm.
pub(crate) fn resolve_server_identity(
    cfg: &ModbusTlsConfig,
    bind_host: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>, bool), TcpError> {
    match &cfg.server_cert {
        ferrowl_util::tls::ServerCertSource::SelfSigned => {
            let (chain, k) = generate_self_signed(bind_host)?;
            Ok((chain, k, false))
        }
        ferrowl_util::tls::ServerCertSource::Explicit {
            cert_file,
            key_file,
        } => {
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
        ferrowl_util::tls::ServerCertSource::Unset => {
            let (chain, k) = generate_self_signed(bind_host)?;
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

/// Build the full `rust_modbus` server TLS config, and whether the self-signed
/// fallback was used without being requested (the caller logs that case).
pub(crate) fn build_server_tls_config(
    cfg: &ModbusTlsConfig,
    bind_host: &str,
) -> Result<(TlsServerConfig, bool), TcpError> {
    let (cert_chain, key, used_fallback) = resolve_server_identity(cfg, bind_host)?;
    let client_certs = if cfg.require_client_cert {
        let ca = cfg.client_ca_file.as_ref().ok_or_else(|| {
            TcpError::Configuration(
                "require_client_cert is set but no client_ca_file was configured (MB-R-108)".into(),
            )
        })?;
        let mut roots = RootStore::empty();
        roots.add_pem(&read_pem(ca)?).map_err(map_tls_err)?;
        ClientCertPolicy::Require(roots)
    } else {
        ClientCertPolicy::None
    };
    Ok((
        TlsServerConfig {
            cert_chain,
            key,
            client_certs,
        },
        used_fallback,
    ))
}

#[cfg(test)]
mod tests {
    use super::ModbusTlsConfig;
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

    fn cert_and_key_pem() -> (String, String) {
        let key = rcgen::KeyPair::generate().expect("keypair generation failed");
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .expect("cert params")
            .self_signed(&key)
            .expect("self-signed cert");
        (cert.pem(), key.serialize_pem())
    }

    /// MB-R-110 — a client identity is presented only when both `client_cert_file` and
    /// `client_key_file` are set.
    #[test]
    fn ut_build_client_tls_config_identity_only_when_both_files_set() {
        use super::build_client_tls_config;

        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem("cert", &cert_pem);
        let key_file = write_pem("key", &key_pem);

        let both = build_client_tls_config(&ModbusTlsConfig {
            client_cert_file: Some(cert_file.clone()),
            client_key_file: Some(key_file.clone()),
            ..Default::default()
        })
        .expect("builds");
        assert!(both.client_identity.is_some());

        let cert_only = build_client_tls_config(&ModbusTlsConfig {
            client_cert_file: Some(cert_file),
            ..Default::default()
        })
        .expect("builds");
        assert!(cert_only.client_identity.is_none());

        let key_only = build_client_tls_config(&ModbusTlsConfig {
            client_key_file: Some(key_file),
            ..Default::default()
        })
        .expect("builds");
        assert!(key_only.client_identity.is_none());

        let neither = build_client_tls_config(&ModbusTlsConfig::default()).expect("builds");
        assert!(neither.client_identity.is_none());
    }

    /// MB-R-109 — `insecure_skip_verify` disables server certificate verification and
    /// ignores `ca_file`, rather than combining the two.
    #[test]
    fn ut_build_client_tls_config_client_verification_skip_wins() {
        use super::build_client_tls_config;
        use ferrowl_util::tls::ClientVerification;
        use rust_modbus::ServerCertVerification;

        let cfg = ModbusTlsConfig {
            client_verification: ClientVerification::resolve(
                true,
                Some("/no/such/ca.pem".to_string()),
            ),
            ..Default::default()
        };
        let built = build_client_tls_config(&cfg).expect("builds despite unreadable ca_file");
        assert!(matches!(
            built.server_cert,
            ServerCertVerification::DangerousDisableVerification
        ));
    }

    /// MB-R-105 — `ModbusTlsConfig` carries the `client_verification`/`server_cert` enums plus
    /// the four untouched fields, all defaulting to their empty/off state.
    #[test]
    fn ut_modbus_tls_config_defaults() {
        use ferrowl_util::tls::{ClientVerification, ServerCertSource};

        let cfg = ModbusTlsConfig::default();
        assert_eq!(cfg.server_cert, ServerCertSource::Unset);
        assert_eq!(
            cfg.client_verification,
            ClientVerification::Verify { ca_file: None }
        );
        assert_eq!(cfg.client_cert_file, None);
        assert_eq!(cfg.client_key_file, None);
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
    }

    /// MB-R-106 — a `SelfSigned` server_cert never reads cert_file/key_file (there are none in
    /// the variant) and is not the fallback (used_fallback == false: it was explicitly asked
    /// for).
    #[test]
    fn ut_resolve_server_identity_self_signed_variant_never_reads_disk() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cfg = ModbusTlsConfig {
            server_cert: ServerCertSource::SelfSigned,
            ..Default::default()
        };
        let (_chain, _key, used_fallback) =
            resolve_server_identity(&cfg, "localhost").expect("self-signed generation succeeds");
        assert!(!used_fallback);
    }

    /// MB-R-106 — an `Explicit` server_cert loads exactly the named cert/key PEM files.
    #[test]
    fn ut_resolve_server_identity_explicit_variant_loads_files() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem("srv-cert", &cert_pem);
        let key_file = write_pem("srv-key", &key_pem);

        let cfg = ModbusTlsConfig {
            server_cert: ServerCertSource::Explicit {
                cert_file,
                key_file,
            },
            ..Default::default()
        };
        let (chain, _key, used_fallback) =
            resolve_server_identity(&cfg, "localhost").expect("loads explicit files");
        assert_eq!(chain.len(), 1);
        assert!(!used_fallback);
    }

    /// MB-R-106 — an `Unset` server_cert falls back to an ephemeral self-signed certificate and
    /// flags the fallback so the caller can log it.
    #[test]
    fn ut_resolve_server_identity_unset_variant_falls_back_and_flags_it() {
        use super::resolve_server_identity;
        use ferrowl_util::tls::ServerCertSource;

        let cfg = ModbusTlsConfig {
            server_cert: ServerCertSource::Unset,
            ..Default::default()
        };
        let (_chain, _key, used_fallback) =
            resolve_server_identity(&cfg, "localhost").expect("falls back to self-signed");
        assert!(used_fallback);
    }
}
