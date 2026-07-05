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
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
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
    /// rig trust a self-signed CSMS certificate without touching the OS trust store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
    /// Client certificate (PEM file path) presented for mutual TLS (Profile 3). Requires
    /// `client_key_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_file: Option<String>,
    /// Private key (PEM file path) matching `client_cert_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_file: Option<String>,
}

impl CsTlsConfig {
    /// Build the rustls-backed [`Connector`]: webpki roots plus `ca_file` if set, and a client
    /// certificate if both `client_cert_file` and `client_key_file` are set.
    pub(crate) fn build_connector(&self) -> Result<Connector, Error> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(path) = &self.ca_file {
            for cert in load_certs(path)? {
                roots.add(cert).map_err(TlsError::Rustls)?;
            }
        }
        let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
        let config = match (&self.client_cert_file, &self.client_key_file) {
            (Some(cert_path), Some(key_path)) => builder
                .with_client_auth_cert(load_certs(cert_path)?, load_private_key(key_path)?)
                .map_err(TlsError::Rustls)?,
            _ => builder.with_no_client_auth(),
        };
        Ok(Connector::Rustls(Arc::new(config)))
    }
}

/// TLS material for the CSMS (server) side (Security Profiles 2 and 3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CsmsTlsConfig {
    /// Server certificate chain (PEM file path).
    pub cert_file: String,
    /// Server private key (PEM file path).
    pub key_file: String,
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
    /// Build the rustls server config from the configured cert/key, adding client-cert
    /// verification when `require_client_cert` is set.
    pub(crate) fn build_server_config(&self) -> Result<Arc<rustls::ServerConfig>, Error> {
        let certs = load_certs(&self.cert_file)?;
        let key = load_private_key(&self.key_file)?;

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
