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

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
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

/// Load a PEM certificate chain from `path`.
fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>, Error> {
    let file = File::open(path).map_err(|source| TlsError::Io {
        path: path.to_owned(),
        source,
    })?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TlsError::Io {
            path: path.to_owned(),
            source,
        })?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificates(path.to_owned()).into());
    }
    Ok(certs)
}

/// Load a PEM private key from `path`.
fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>, Error> {
    let file = File::open(path).map_err(|source| TlsError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|source| TlsError::Io {
            path: path.to_owned(),
            source,
        })?
        .ok_or_else(|| TlsError::NoPrivateKey(path.to_owned()).into())
}
