//! Security-profile configuration shared by CS and CSMS.
//!
//! OCPP-J security profiles this crate supports:
//! * Profile 1 -- HTTP Basic Auth over plain `ws://`.
//! * Profile 2 -- TLS (`wss://`), server certificate only.
//! * Profile 3 -- mutual TLS: the peer also presents (and the other side verifies) a client
//!   certificate.
//!
//! Profiles 2 and 3 share the same rustls plumbing; a [`CsTlsConfig`]/[`CsmsTlsConfig`] only
//! becomes "profile 3" once a client certificate (CS) or `require_client_cert` (CSMS) is set.

use std::sync::Arc;

use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::{self, PemObject};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::error::{Error, TlsError};

/// HTTP Basic Auth credentials (Security Profile 1).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for BasicAuth {
    /// Redacts the password so it never lands in a log line via `{:?}`.
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("BasicAuth")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl BasicAuth {
    /// The `Authorization: Basic ...` header value for this credential pair.
    pub(crate) fn header_value(&self) -> HeaderValue {
        let raw = format!("{}:{}", self.username, self.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
        HeaderValue::from_str(&format!("Basic {encoded}"))
            .expect("base64 alphabet is always valid header content")
    }

    /// Whether `header` (the raw `Authorization` header value received on a handshake request)
    /// matches these credentials. Missing header never matches.
    pub(crate) fn matches(&self, header: Option<&HeaderValue>) -> bool {
        header
            .map(|h| h.as_bytes() == self.header_value().as_bytes())
            .unwrap_or(false)
    }
}

/// TLS material for the CS (client) side (Security Profiles 2 and 3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CsTlsConfig {
    /// Extra trust anchor (PEM file path) added on top of the webpki root store -- lets a test
    /// rig trust a self-signed CSMS certificate without touching the OS trust store. Ignored (see
    /// `insecure_skip_verify`) when that flag is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
    /// Client certificate (PEM file path) presented for mutual TLS (Profile 3). Requires
    /// `client_key_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_file: Option<String>,
    /// Private key (PEM file path) matching `client_cert_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_file: Option<String>,
    /// Accept *any* server certificate, skipping authentication entirely -- the connection stays
    /// TLS-encrypted, but the peer's identity is not checked, so it is vulnerable to a
    /// man-in-the-middle. Only meaningful against a CSMS using an ephemeral self-signed
    /// certificate (whose identity changes every start and so cannot be pinned via `ca_file`).
    /// **Test rigs only -- never enable this against a production CSMS.** When set, `ca_file` is
    /// ignored rather than combined with it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub insecure_skip_verify: bool,
}

impl CsTlsConfig {
    /// Build the rustls-backed [`Connector`]: webpki roots plus `ca_file` if set, and a client
    /// certificate if both `client_cert_file` and `client_key_file` are set. When
    /// `insecure_skip_verify` is set, server authentication is disabled entirely and `ca_file` is
    /// ignored.
    pub(crate) fn build_connector(&self) -> Result<Connector, Error> {
        let builder = rustls::ClientConfig::builder();
        let builder = if self.insecure_skip_verify {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert::new()))
        } else {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(path) = &self.ca_file {
                for cert in load_certs(path)? {
                    roots.add(cert).map_err(TlsError::Rustls)?;
                }
            }
            builder.with_root_certificates(roots)
        };
        let config = match (&self.client_cert_file, &self.client_key_file) {
            (Some(cert_path), Some(key_path)) => builder
                .with_client_auth_cert(load_certs(cert_path)?, load_private_key(key_path)?)
                .map_err(TlsError::Rustls)?,
            _ => builder.with_no_client_auth(),
        };
        Ok(Connector::Rustls(Arc::new(config)))
    }
}

/// A [`ServerCertVerifier`] that accepts any server certificate without checking it. The
/// connection remains TLS-encrypted (confidential, integrity-protected) but the peer is not
/// authenticated at all -- suitable only for talking to a CSMS whose ephemeral self-signed
/// certificate cannot be pinned in advance. Signature verification itself is still delegated to
/// the default crypto provider so the handshake is cryptographically sound; only the
/// certificate-chain/identity check is skipped.
#[derive(Debug)]
struct AcceptAnyServerCert {
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl AcceptAnyServerCert {
    fn new() -> Self {
        Self {
            supported_algs: CryptoProvider::get_default()
                .expect("a default rustls CryptoProvider is installed")
                .signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}

/// Where the CSMS TLS server certificate/key come from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CsmsTlsMode {
    /// Certificate chain and private key loaded from PEM files on disk.
    Files { cert_file: String, key_file: String },
    /// An ephemeral self-signed certificate, generated in memory fresh at each server start and
    /// never written to disk. Since the identity changes every start, connecting clients cannot
    /// pin it in advance -- see `CsTlsConfig::insecure_skip_verify`. Not compatible with
    /// `require_client_cert` (there is no CA to hand out to clients for mTLS in this mode).
    SelfSigned,
}

/// TLS material for the CSMS (server) side (Security Profiles 2 and 3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CsmsTlsConfig {
    /// Source of the server certificate chain and private key.
    pub mode: CsmsTlsMode,
    /// CA (PEM file path) used to verify client certificates. Required when
    /// `require_client_cert` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ca_file: Option<String>,
    /// Reject client connections that fail to present a certificate signed by `client_ca_file`
    /// (Security Profile 3).
    #[serde(default)]
    pub require_client_cert: bool,
}

impl CsmsTlsConfig {
    /// Build the rustls server config from the configured cert/key (loaded from disk, or an
    /// ephemeral self-signed pair generated in memory), adding client-cert verification when
    /// `require_client_cert` is set. `host` is the listener's bind address/hostname, included as
    /// a SAN entry when generating a self-signed certificate.
    pub(crate) fn build_server_config(
        &self,
        host: &str,
    ) -> Result<Arc<rustls::ServerConfig>, Error> {
        let (certs, key) = match &self.mode {
            CsmsTlsMode::Files {
                cert_file,
                key_file,
            } => (load_certs(cert_file)?, load_private_key(key_file)?),
            CsmsTlsMode::SelfSigned => {
                if self.require_client_cert {
                    return Err(TlsError::SelfSignedWithClientCert.into());
                }
                generate_self_signed(host)?
            }
        };

        let builder = rustls::ServerConfig::builder();
        let builder = if self.require_client_cert {
            let ca_file = self
                .client_ca_file
                .as_ref()
                .ok_or(TlsError::MissingClientCa)?;
            let mut roots = rustls::RootCertStore::empty();
            for cert in load_certs(ca_file)? {
                roots.add(cert).map_err(TlsError::Rustls)?;
            }
            let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| TlsError::ClientVerifier(e.to_string()))?;
            builder.with_client_cert_verifier(verifier)
        } else {
            builder.with_no_client_auth()
        };

        let config = builder
            .with_single_cert(certs, key)
            .map_err(TlsError::Rustls)?;
        Ok(Arc::new(config))
    }
}

/// Generate an ephemeral self-signed certificate/key pair in memory (never written to disk), with
/// `host` and `"localhost"` as SAN entries and CN `"ferrowl CSMS"`.
fn generate_self_signed(
    host: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), Error> {
    let mut names = vec![host.to_string()];
    if host != "localhost" {
        names.push("localhost".to_string());
    }
    let mut params = rcgen::CertificateParams::new(names).map_err(TlsError::SelfSignedGen)?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "ferrowl CSMS");

    let key_pair = rcgen::KeyPair::generate().map_err(TlsError::SelfSignedGen)?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(TlsError::SelfSignedGen)?;

    let cert_der = cert.der().clone();
    let key_der = PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(TlsError::SelfSignedKeyEncoding)?;
    Ok((vec![cert_der], key_der))
}

/// Map a PEM failure onto the `io::Error` shape the rest of this module already speaks. A file
/// that cannot be opened arrives as `pem::Error::Io` and is passed through unchanged; every other
/// variant is a parse failure, which the previous `rustls-pemfile` codec also surfaced as an
/// `InvalidData` `io::Error`.
fn pem_io_error(err: pem::Error) -> std::io::Error {
    match err {
        pem::Error::Io(io) => io,
        other => std::io::Error::new(std::io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// Load a PEM certificate chain from `path`.
fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>, Error> {
    let certs = CertificateDer::pem_file_iter(path)
        .map_err(|source| TlsError::Io {
            path: path.to_owned(),
            source: pem_io_error(source),
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TlsError::Io {
            path: path.to_owned(),
            source: pem_io_error(source),
        })?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificates(path.to_owned()).into());
    }
    Ok(certs)
}

/// Load a PEM private key from `path`.
fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>, Error> {
    match PrivateKeyDer::from_pem_file(path) {
        Ok(key) => Ok(key),
        Err(pem::Error::NoItemsFound) => Err(TlsError::NoPrivateKey(path.to_owned()).into()),
        Err(source) => Err(TlsError::Io {
            path: path.to_owned(),
            source: pem_io_error(source),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Write `contents` to a fresh file under the OS temp dir and return its path. Left in place;
    /// the platform reclaims its temp dir, and this keeps the tests free of a TempDir dependency.
    fn temp_pem(tag: &str, contents: &str) -> String {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrowl-ocpp-sec-{tag}-{}-{n}.pem",
            std::process::id()
        ));
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(contents.as_bytes()))
            .expect("write temp pem");
        path.to_string_lossy().into_owned()
    }

    /// A self-signed certificate and its matching key, both PEM-encoded (PKCS#8 key, as rcgen
    /// emits and as the loopback tests feed the real TLS stack).
    fn cert_and_key_pem() -> (String, String) {
        let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    /// OC-R-041 — a well-formed certificate and key load, pinning the happy path the negative
    /// cases below are measured against.
    #[test]
    fn ut_load_certs_and_key_roundtrip() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_path = temp_pem("roundtrip-cert", &cert_pem);
        let key_path = temp_pem("roundtrip-key", &key_pem);
        assert_eq!(load_certs(&cert_path).unwrap().len(), 1);
        load_private_key(&key_path).expect("key should load");
    }

    /// OC-R-041 — failing to open a certificate file is a TLS error, not a panic or a silent
    /// empty chain.
    #[test]
    fn ut_load_certs_missing_file_is_tls_error() {
        let missing = temp_pem("missing-cert", "");
        std::fs::remove_file(&missing).unwrap();
        assert!(matches!(
            load_certs(&missing),
            Err(Error::Tls(TlsError::Io { .. }))
        ));
    }

    /// OC-R-041 — a readable file with no certificate section fails as NoCertificates rather than
    /// being accepted as an empty chain.
    #[test]
    fn ut_load_certs_without_certificate_section_is_no_certificates() {
        let (_cert, key_pem) = cert_and_key_pem();
        let path = temp_pem("key-only", &key_pem);
        assert!(matches!(
            load_certs(&path),
            Err(Error::Tls(TlsError::NoCertificates(_)))
        ));
    }

    /// OC-R-041 — failing to find a private key in a readable file is NoPrivateKey.
    #[test]
    fn ut_load_private_key_without_key_section_is_no_private_key() {
        let (cert_pem, _key) = cert_and_key_pem();
        let path = temp_pem("cert-only", &cert_pem);
        assert!(matches!(
            load_private_key(&path),
            Err(Error::Tls(TlsError::NoPrivateKey(_)))
        ));
    }
}
