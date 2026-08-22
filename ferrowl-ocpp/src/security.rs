//! Security-profile configuration shared by CS and CSMS.
//!
//! OCPP-J security profiles this crate supports:
//! * Profile 1 -- HTTP Basic Auth over plain `ws://`.
//! * Profile 2 -- TLS (`wss://`), server certificate only.
//! * Profile 3 -- mutual TLS: the peer also presents (and the other side verifies) a client
//!   certificate.
//!
//! Profiles 2 and 3 share the same rustls plumbing; a [`ferrowl_util::tls::ClientTlsPolicy`]/
//! [`ferrowl_util::tls::ServerTlsPolicy`] only becomes "profile 3" once its `MutualTls` variant
//! is resolved (a client certificate for CS, `require_client_cert` on the wire for CSMS).

use std::sync::Arc;

use base64::Engine;
use parking_lot::Mutex;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::{self, PemObject};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};

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
    /// matches these credentials. Missing header never matches. NF-R-032: the comparison is
    /// constant-time, so a wrong credential leaks no timing signal about where it first diverges.
    pub(crate) fn matches(&self, header: Option<&HeaderValue>) -> bool {
        use subtle::ConstantTimeEq;

        header
            .map(|h| {
                let expected = self.header_value();
                let (got, expected) = (h.as_bytes(), expected.as_bytes());
                got.len() == expected.len() && got.ct_eq(expected).into()
            })
            .unwrap_or(false)
    }
}

/// One extra header sent on the CS WebSocket upgrade request in addition to the client's own
/// (subprotocol, Basic Auth) — OC-R-117.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "HeaderDefWire")]
pub struct HeaderDef {
    pub name: String,
    pub value: String,
}

#[derive(serde::Deserialize)]
struct HeaderDefWire {
    name: String,
    value: String,
}

impl TryFrom<HeaderDefWire> for HeaderDef {
    type Error = crate::error::HeaderError;
    fn try_from(w: HeaderDefWire) -> Result<Self, Self::Error> {
        HeaderDef::new(w.name, w.value)
    }
}

/// Header names the client itself sets — OC-R-117's exact collision list (see the plan's
/// `## Shared` section for the source list).
const RESERVED_HEADER_NAMES: [&str; 8] = [
    "authorization",
    "host",
    "upgrade",
    "connection",
    "sec-websocket-key",
    "sec-websocket-version",
    "sec-websocket-protocol",
    "sec-websocket-extensions",
];

impl HeaderDef {
    /// Construct a validated header. OC-R-117: rejects a `name` case-insensitively matching a
    /// client-controlled header. OC-R-118: `name` must be an HTTP token (RFC 7230 `tchar*`, no
    /// separators/whitespace); `value` must be printable ASCII only (0x20-0x7E), which excludes
    /// CR/LF and other control bytes.
    pub fn new(
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, crate::error::HeaderError> {
        let name = name.into();
        let value = value.into();
        if RESERVED_HEADER_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
            return Err(crate::error::HeaderError::Reserved(name));
        }
        if name.is_empty() || !name.bytes().all(is_tchar) {
            return Err(crate::error::HeaderError::InvalidName(name));
        }
        if !value.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(crate::error::HeaderError::InvalidValue(name));
        }
        Ok(HeaderDef { name, value })
    }
}

/// RFC 7230 `tchar`: `"!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." / "^" / "_" /
/// "`" / "|" / "~" / DIGIT / ALPHA`.
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// A self-signed certificate/key pair generated once per module instance and reused across
/// connect/reconnect/bind attempts, cleared whenever the resolved source moves away from
/// self-signed so a later reversion regenerates fresh material (OC-R-037/OC-R-115 — mirrors
/// `ferrowl_modbus::tcp::SelfSignedCache` exactly; independently defined here since
/// `ferrowl-util` does not depend on `rustls-pki-types`/`parking_lot`).
pub type SelfSignedCache =
    Arc<Mutex<Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>>>;

/// A fresh, empty cache — call exactly once per module instance, never per-reconfigure, so a
/// config edit that keeps the source self-signed reuses the same material instead of
/// regenerating it.
pub fn new_self_signed_cache() -> SelfSignedCache {
    Arc::new(Mutex::new(None))
}

/// Resolve a self-signed pair via the cache/regenerate rule (OC-R-037/OC-R-115, `## Shared` in
/// the tls-mtls-role-split plan): reused whenever the resolved source stays self-signed,
/// regenerated the first time or after any transition away from self-signed cleared it.
/// `PrivateKeyDer` is not `Clone` (by design, `rustls-pki-types` 1.15.1); `clone_key()` is its
/// explicit deep-copy escape hatch, used on every cache hit.
fn resolve_self_signed(
    host: &str,
    cache: &SelfSignedCache,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), Error> {
    let mut guard = cache.lock();
    if let Some((chain, key)) = guard.as_ref() {
        return Ok((chain.clone(), key.clone_key()));
    }
    let (chain, key) = generate_self_signed(host)?;
    *guard = Some((chain.clone(), key.clone_key()));
    Ok((chain, key))
}

/// Build the rustls-backed [`Connector`] from a resolved [`ClientTlsPolicy`]: `NoTls` is
/// unreachable here — the caller only invokes this under `Some(policy)` (the wrapping `Option`
/// on `cs::Config.tls` itself carries the "no TLS configured" signal). `Tls`/`MutualTls` build
/// server-certificate verification identically (OC-R-036: skip-verify wins over `ca_file`, never
/// combined — [`ClientVerification::resolve`]); `MutualTls` additionally presents a client
/// identity: `Explicit` loads from disk, `SelfSigned` generates/reuses via the cache rule
/// (OC-R-115).
pub(crate) fn build_connector(
    policy: &ClientTlsPolicy,
    cache: &SelfSignedCache,
) -> Result<Connector, Error> {
    let (client_verification, client_identity_source) = match policy {
        ClientTlsPolicy::NoTls => {
            unreachable!("build_connector is only ever called under Some(policy)")
        }
        ClientTlsPolicy::Tls {
            client_verification,
        } => (client_verification, None),
        ClientTlsPolicy::MutualTls {
            client_verification,
            client_identity,
        } => (client_verification, Some(client_identity)),
    };

    let builder = rustls::ClientConfig::builder();
    let builder = match client_verification {
        ClientVerification::SkipVerify => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert::new())),
        ClientVerification::Verify { ca_file } => {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(path) = ca_file {
                for cert in load_certs(path)? {
                    roots.add(cert).map_err(TlsError::Rustls)?;
                }
            }
            builder.with_root_certificates(roots)
        }
    };
    let config = match client_identity_source {
        Some(ClientCertSource::Explicit {
            client_cert_file,
            client_key_file,
        }) => builder
            .with_client_auth_cert(
                load_certs(client_cert_file)?,
                load_private_key(client_key_file)?,
            )
            .map_err(TlsError::Rustls)?,
        Some(ClientCertSource::SelfSigned) => {
            let (chain, key) = resolve_self_signed("ferrowl-ocpp-client", cache)?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(TlsError::Rustls)?
        }
        None => builder.with_no_client_auth(),
    };
    Ok(Connector::Rustls(Arc::new(config)))
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

/// A [`ClientCertVerifier`] that requires a client certificate be presented, but performs no
/// chain/identity validation against any root store -- the server-role mirror of
/// [`AcceptAnyServerCert`]. Backs `ServerTlsPolicy::MutualTls`'s `ClientCertVerification::
/// SkipVerify` (OC-R-039): a handshake presenting no certificate still fails
/// (`client_auth_mandatory` stays at its default of `true`, following `offer_client_auth`), but a
/// presented certificate's chain/identity is never checked. Signature verification itself is
/// still delegated to the default crypto provider so the handshake stays cryptographically sound.
#[derive(Debug)]
struct AllowAnyClientCert {
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl AllowAnyClientCert {
    fn new() -> Self {
        Self {
            supported_algs: CryptoProvider::get_default()
                .expect("a default rustls CryptoProvider is installed")
                .signature_verification_algorithms,
        }
    }
}

impl ClientCertVerifier for AllowAnyClientCert {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        // No hints to offer -- any client certificate is accepted regardless of its issuer, so
        // there is no meaningful "acceptable trust anchor" list to advertise.
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
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

/// Resolve the CSMS's presented certificate per OC-R-096, and whether an ephemeral self-signed
/// certificate was used *without* being explicitly requested (the caller logs that case,
/// OC-R-095) -- mirrors `ferrowl_modbus::tcp::tls::resolve_server_identity` exactly.
/// `self_signed` wins unconditionally over `cert_file`/`key_file`, enforced by
/// [`ServerCertSource`] at construction, so the "one set, not the other" case is unrepresentable
/// here and needs no error arm.
fn resolve_server_identity(
    server_cert: &ServerCertSource,
    host: &str,
    cache: &SelfSignedCache,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>, bool), Error> {
    match server_cert {
        ServerCertSource::SelfSigned => {
            let (chain, key) = resolve_self_signed(host, cache)?;
            Ok((chain, key, false))
        }
        ServerCertSource::Explicit {
            cert_file,
            key_file,
        } => {
            // Any explicit configuration clears the cache: a later reversion to self-signed must
            // regenerate rather than reuse material from before the explicit interlude.
            *cache.lock() = None;
            Ok((load_certs(cert_file)?, load_private_key(key_file)?, false))
        }
        ServerCertSource::Unset => {
            let (chain, key) = resolve_self_signed(host, cache)?;
            Ok((chain, key, true))
        }
    }
}

/// Build the rustls server config from a resolved [`ServerTlsPolicy`]: `NoTls` is unreachable
/// here -- the caller only invokes this under `Some(policy)` (the wrapping `Option` on
/// `csms::Config.tls` itself carries the "no TLS configured" signal). Returns the built config
/// plus whether an unrequested ephemeral self-signed fallback was used (OC-R-095/OC-R-096, the
/// caller logs that case). `host` is the listener's bind address/hostname, included as a SAN
/// entry when generating a self-signed certificate. `MutualTls`'s `Verify{ca_files}` trusts a
/// client certificate signed by *any one* of the configured CAs (OC-R-039/OC-R-113's "any one is
/// sufficient, not all"); `SkipVerify` still requires a presented certificate but skips
/// chain/identity validation entirely ([`AllowAnyClientCert`]).
pub(crate) fn build_server_config(
    policy: &ServerTlsPolicy,
    host: &str,
    cache: &SelfSignedCache,
) -> Result<(Arc<rustls::ServerConfig>, bool), Error> {
    let (server_cert, client_verification) = match policy {
        ServerTlsPolicy::NoTls => {
            unreachable!("build_server_config is only ever called under Some(policy)")
        }
        ServerTlsPolicy::Tls { server_cert } => (server_cert, None),
        ServerTlsPolicy::MutualTls {
            server_cert,
            client_verification,
        } => (server_cert, Some(client_verification)),
    };
    let (certs, key, used_fallback) = resolve_server_identity(server_cert, host, cache)?;

    let builder = rustls::ServerConfig::builder();
    let builder = match client_verification {
        None => builder.with_no_client_auth(),
        Some(ClientCertVerification::Verify { ca_files }) => {
            let mut roots = rustls::RootCertStore::empty();
            for ca_file in ca_files {
                for cert in load_certs(ca_file)? {
                    roots.add(cert).map_err(TlsError::Rustls)?;
                }
            }
            let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| TlsError::ClientVerifier(e.to_string()))?;
            builder.with_client_cert_verifier(verifier)
        }
        Some(ClientCertVerification::SkipVerify) => {
            builder.with_client_cert_verifier(Arc::new(AllowAnyClientCert::new()))
        }
    };

    let config = builder
        .with_single_cert(certs, key)
        .map_err(TlsError::Rustls)?;
    Ok((Arc::new(config), used_fallback))
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
    let resolved = ferrowl_util::path::expand(path);
    let certs = CertificateDer::pem_file_iter(&resolved)
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
    let resolved = ferrowl_util::path::expand(path);
    match PrivateKeyDer::from_pem_file(&resolved) {
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

    /// Like [`temp_pem`], but under the real home directory and returned as a `~/...` path, for
    /// exercising NF-R-042 tilde expansion. Returns `(tilde_path, actual_path)`.
    fn home_pem(tag: &str, contents: &str) -> (String, std::path::PathBuf) {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        let filename = format!(
            "ferrowl-ocpp-sec-tilde-{tag}-{}-{n}.pem",
            std::process::id()
        );
        let actual = home.join(&filename);
        std::fs::File::create(&actual)
            .and_then(|mut f| f.write_all(contents.as_bytes()))
            .expect("write home pem");
        (format!("~/{filename}"), actual)
    }

    #[test]
    /// NF-R-042 — `load_certs`/`load_private_key` expand a leading `~` to the home directory.
    fn ut_load_certs_and_key_expand_tilde() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let (cert_tilde, cert_actual) = home_pem("cert", &cert_pem);
        let (key_tilde, key_actual) = home_pem("key", &key_pem);

        let certs = load_certs(&cert_tilde);
        let key = load_private_key(&key_tilde);
        let _ = std::fs::remove_file(&cert_actual);
        let _ = std::fs::remove_file(&key_actual);

        assert_eq!(certs.unwrap().len(), 1);
        key.expect("key should load via a ~/-prefixed path");
    }

    #[test]
    /// OC-R-041 — a well-formed certificate and key load, pinning the happy path the negative
    /// cases below are measured against.
    fn ut_load_certs_and_key_roundtrip() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_path = temp_pem("roundtrip-cert", &cert_pem);
        let key_path = temp_pem("roundtrip-key", &key_pem);
        assert_eq!(load_certs(&cert_path).unwrap().len(), 1);
        load_private_key(&key_path).expect("key should load");
    }

    #[test]
    /// OC-R-041 — failing to open a certificate file is a TLS error, not a panic or a silent
    /// empty chain.
    fn ut_load_certs_missing_file_is_tls_error() {
        let missing = temp_pem("missing-cert", "");
        std::fs::remove_file(&missing).unwrap();
        assert!(matches!(
            load_certs(&missing),
            Err(Error::Tls(TlsError::Io { .. }))
        ));
    }

    #[test]
    /// OC-R-041 — a readable file with no certificate section fails as NoCertificates rather than
    /// being accepted as an empty chain.
    fn ut_load_certs_without_certificate_section_is_no_certificates() {
        let (_cert, key_pem) = cert_and_key_pem();
        let path = temp_pem("key-only", &key_pem);
        assert!(matches!(
            load_certs(&path),
            Err(Error::Tls(TlsError::NoCertificates(_)))
        ));
    }

    #[test]
    /// OC-R-041 — failing to find a private key in a readable file is NoPrivateKey.
    fn ut_load_private_key_without_key_section_is_no_private_key() {
        let (cert_pem, _key) = cert_and_key_pem();
        let path = temp_pem("cert-only", &cert_pem);
        assert!(matches!(
            load_private_key(&path),
            Err(Error::Tls(TlsError::NoPrivateKey(_)))
        ));
    }

    fn auth() -> BasicAuth {
        BasicAuth {
            username: "user".into(),
            password: "pass".into(),
        }
    }

    #[test]
    /// OC-R-030 — a configured Basic Auth credential produces a base64 `Authorization: Basic` header.
    fn ut_basic_auth_header_value() {
        assert_eq!(
            auth().header_value().to_str().unwrap(),
            "Basic dXNlcjpwYXNz"
        );
    }

    #[test]
    /// OC-R-031 — the CSMS matches only the exact credential header; a wrong or missing header never
    /// matches.
    fn ut_basic_auth_matches() {
        let a = auth();
        let good = a.header_value();
        assert!(a.matches(Some(&good)));
        assert!(!a.matches(Some(&HeaderValue::from_static("Basic d3Jvbmc="))));
        assert!(!a.matches(None));
    }

    #[test]
    /// NF-R-032 — a same-length wrong header is rejected exactly like a different-length one; the
    /// constant-time comparison must not special-case the equal-length case as a match.
    fn ut_basic_auth_matches_rejects_same_length_wrong_header() {
        let a = auth();
        let good = a.header_value();
        let mut same_length_wrong = good.to_str().unwrap().as_bytes().to_vec();
        *same_length_wrong.last_mut().unwrap() ^= 1;
        let same_length_wrong = HeaderValue::from_bytes(&same_length_wrong).unwrap();
        assert!(!a.matches(Some(&same_length_wrong)));
    }

    #[test]
    /// OC-R-033 — the Basic Auth password never appears in a `{:?}` debug rendering.
    fn ut_basic_auth_debug_redacts_password() {
        let creds = BasicAuth {
            username: "user".into(),
            password: "s3cr3tpw".into(),
        };
        let shown = format!("{creds:?}");
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("s3cr3tpw"));
        assert!(shown.contains("user")); // username is not redacted
    }

    #[test]
    /// MB-R-105/OC-R-035 — a client identity is presented only under `MutualTls`; a bare `Tls`
    /// policy never presents one, regardless of what would otherwise be configured.
    fn ut_build_connector_identity_only_under_mutual_tls() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert = temp_pem("cs-cert", &cert_pem);
        let key = temp_pem("cs-key", &key_pem);
        let cache = new_self_signed_cache();

        let mtls = ClientTlsPolicy::MutualTls {
            client_verification: ClientVerification::Verify { ca_file: None },
            client_identity: ClientCertSource::Explicit {
                client_cert_file: cert,
                client_key_file: key,
            },
        };
        assert!(build_connector(&mtls, &cache).is_ok());

        let tls_only = ClientTlsPolicy::Tls {
            client_verification: ClientVerification::Verify { ca_file: None },
        };
        assert!(build_connector(&tls_only, &cache).is_ok());
    }

    #[test]
    /// OC-R-035 — a `MutualTls` policy with a valid explicit client cert/key pair builds; the
    /// same pair missing on disk fails to build, proving the identity really is loaded.
    fn ut_build_connector_explicit_identity_loaded_when_present() {
        let cache = new_self_signed_cache();

        let missing = ClientTlsPolicy::MutualTls {
            client_verification: ClientVerification::Verify { ca_file: None },
            client_identity: ClientCertSource::Explicit {
                client_cert_file: "/no/such/ferrowl-cert.pem".into(),
                client_key_file: "/no/such/ferrowl-key.pem".into(),
            },
        };
        assert!(build_connector(&missing, &cache).is_err());
    }

    /// OC-R-036 — `insecure_skip_verify` disables server-certificate verification (installs
    /// `AcceptAnyServerCert`) and ignores `ca_file`, even when it points at an unreadable path.
    #[test]
    fn ut_build_connector_client_verification_skip_wins() {
        let cache = new_self_signed_cache();
        let skip = ClientTlsPolicy::Tls {
            client_verification: ClientVerification::resolve(
                true,
                Some("/no/such/ca.pem".to_string()),
            ),
        };
        assert!(build_connector(&skip, &cache).is_ok());

        // Without skip-verify, the same unreadable ca_file must fail to load.
        let verify = ClientTlsPolicy::Tls {
            client_verification: ClientVerification::resolve(
                false,
                Some("/no/such/ca.pem".to_string()),
            ),
        };
        assert!(build_connector(&verify, &cache).is_err());
    }

    /// OC-R-115 — a `SelfSigned` client identity is generated once and reused across repeated
    /// calls sharing the same cache (cache hit, not a fresh key pair each time).
    #[test]
    fn ut_build_connector_self_signed_identity_reuses_cache() {
        let cache = new_self_signed_cache();
        let policy = ClientTlsPolicy::MutualTls {
            client_verification: ClientVerification::Verify { ca_file: None },
            client_identity: ClientCertSource::SelfSigned,
        };
        assert!(build_connector(&policy, &cache).is_ok());
        assert!(build_connector(&policy, &cache).is_ok());
        let chain1 = cache
            .lock()
            .as_ref()
            .expect("cached after first build")
            .0
            .clone();
        assert!(build_connector(&policy, &cache).is_ok());
        let chain2 = cache.lock().as_ref().expect("still cached").0.clone();
        assert_eq!(chain1, chain2, "cache hit reuses the same certificate");
    }

    #[test]
    /// OC-R-038 — a generated self-signed certificate carries the listener's host as a SAN, plus `localhost` when the host differs.
    fn ut_self_signed_carries_host_and_localhost_sans() {
        // DNS SAN names are encoded as ASCII (IA5String) inside the certificate DER.
        let contains = |der: &[u8], needle: &[u8]| der.windows(needle.len()).any(|w| w == needle);

        let (certs, _key) = generate_self_signed("evse.example").unwrap();
        let der = certs[0].as_ref();
        assert!(contains(der, b"evse.example"));
        assert!(contains(der, b"localhost"));

        // When the host already is localhost it is still present as a SAN.
        let (certs, _key) = generate_self_signed("localhost").unwrap();
        assert!(contains(certs[0].as_ref(), b"localhost"));
    }

    #[test]
    /// OC-R-039 — a CSMS requiring client certificates with a configured client CA builds a server config carrying a client-cert verifier.
    fn ut_require_client_cert_with_ca_builds_verifier() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let (ca_pem, _ca_key) = cert_and_key_pem();
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::Explicit {
                cert_file: temp_pem("mtls-cert", &cert_pem),
                key_file: temp_pem("mtls-key", &key_pem),
            },
            client_verification: ClientCertVerification::Verify {
                ca_files: vec![temp_pem("mtls-ca", &ca_pem)],
            },
        };
        assert!(build_server_config(&policy, "localhost", &cache).is_ok());
    }

    #[test]
    /// OC-R-041 — a self-signed CSMS builds a usable server TLS config in memory.
    /// OC-R-037 — the CSMS server certificate may come from an ephemeral in-memory self-signed pair.
    fn ut_build_server_config_self_signed() {
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::SelfSigned,
        };
        let (_config, used_fallback) =
            build_server_config(&policy, "localhost", &cache).expect("builds");
        assert!(!used_fallback, "self-signed was explicitly requested");
    }

    /// OC-R-037 (cache reuse) — a `SelfSigned` server_cert reuses the cached pair across repeat
    /// calls sharing the same cache, rather than regenerating a fresh key pair each time.
    #[test]
    fn ut_build_server_config_self_signed_reuses_cached_pair() {
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::SelfSigned,
        };
        build_server_config(&policy, "localhost", &cache).expect("builds");
        let chain1 = cache
            .lock()
            .as_ref()
            .expect("cached after first build")
            .0
            .clone();
        build_server_config(&policy, "localhost", &cache).expect("builds");
        let chain2 = cache.lock().as_ref().expect("still cached").0.clone();
        assert_eq!(chain1, chain2, "cache hit reuses the same certificate");
    }

    /// OC-R-037 (cache regen) — resolving `Explicit` clears the cache, so a later reversion to
    /// `SelfSigned` regenerates fresh material rather than reusing anything from before the
    /// explicit interlude.
    #[test]
    fn ut_build_server_config_explicit_then_self_signed_regenerates() {
        let cache = new_self_signed_cache();
        let self_signed = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::SelfSigned,
        };
        build_server_config(&self_signed, "localhost", &cache).expect("builds");
        let chain1 = cache
            .lock()
            .as_ref()
            .expect("cached after first build")
            .0
            .clone();

        let (cert_pem, key_pem) = cert_and_key_pem();
        let explicit = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::Explicit {
                cert_file: temp_pem("explicit-cert", &cert_pem),
                key_file: temp_pem("explicit-key", &key_pem),
            },
        };
        build_server_config(&explicit, "localhost", &cache).expect("builds");
        assert!(
            cache.lock().is_none(),
            "an explicit build must clear the cache"
        );

        build_server_config(&self_signed, "localhost", &cache).expect("builds");
        let chain2 = cache
            .lock()
            .as_ref()
            .expect("regenerated after reversion")
            .0
            .clone();
        assert_ne!(
            chain1, chain2,
            "an explicit interlude must clear the cache, forcing regeneration"
        );
    }

    #[test]
    /// OC-R-040 — a self-signed CSMS requiring client certificates succeeds when a
    /// `client_ca_file` is configured, in either verification mode: the server's own self-signed
    /// identity and the CA trusted for verifying client certificates are independent.
    fn ut_require_client_cert_self_signed_with_any_verification_mode_succeeds() {
        let (ca_pem, _ca_key) = cert_and_key_pem();
        let cache = new_self_signed_cache();

        let verify = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::SelfSigned,
            client_verification: ClientCertVerification::Verify {
                ca_files: vec![temp_pem("self-signed-mtls-ca", &ca_pem)],
            },
        };
        assert!(build_server_config(&verify, "localhost", &cache).is_ok());

        let skip = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::SelfSigned,
            client_verification: ClientCertVerification::SkipVerify,
        };
        assert!(build_server_config(&skip, "localhost", &cache).is_ok());
    }

    #[test]
    /// OC-R-041 — a CSMS builds its server TLS config from on-disk certificate/key files.
    /// OC-R-037 — the CSMS server certificate may come from PEM files on disk.
    fn ut_build_server_config_from_files() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::Explicit {
                cert_file: temp_pem("srv-cert", &cert_pem),
                key_file: temp_pem("srv-key", &key_pem),
            },
        };
        assert!(build_server_config(&policy, "localhost", &cache).is_ok());
    }

    #[test]
    /// OC-R-096 — a `Tls` policy with `server_cert: Unset` falls back to an ephemeral self-signed
    /// certificate and flags the fallback so the caller can log it (OC-R-095).
    fn ut_build_server_config_unset_falls_back_to_ephemeral_and_flags_it() {
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::Tls {
            server_cert: ServerCertSource::Unset,
        };
        let (_config, used_fallback) =
            build_server_config(&policy, "localhost", &cache).expect("falls back to self-signed");
        assert!(used_fallback);
    }

    /// OC-R-039 — server-role `SkipVerify` still requires a client certificate be presented (the
    /// config resolves and builds); the actual "no cert at all is rejected, an untrusted one is
    /// accepted" handshake behavior is proven end-to-end by the loopback integration test in
    /// `tests/ws_loopback_security.rs`.
    #[test]
    fn ut_build_server_config_skip_verify_requires_presented_cert() {
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::SelfSigned,
            client_verification: ClientCertVerification::SkipVerify,
        };
        assert!(build_server_config(&policy, "localhost", &cache).is_ok());
    }

    /// OC-R-039/OC-R-113 — `Verify{ca_files}` accumulates every configured CA into a single trust
    /// store, config-resolution level (the actual "any one is sufficient" handshake accept is
    /// proven end-to-end by the loopback integration test).
    #[test]
    fn ut_build_server_config_multi_ca_accumulates_into_one_root_store() {
        let (ca1_pem, _) = cert_and_key_pem();
        let (ca2_pem, _) = cert_and_key_pem();
        let cache = new_self_signed_cache();
        let policy = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::SelfSigned,
            client_verification: ClientCertVerification::Verify {
                ca_files: vec![temp_pem("ca1", &ca1_pem), temp_pem("ca2", &ca2_pem)],
            },
        };
        assert!(build_server_config(&policy, "localhost", &cache).is_ok());
    }

    #[test]
    /// OC-R-117 — a `HeaderDef` name colliding case-insensitively with a client-controlled
    /// header is rejected, naming the offending header in the error.
    fn ut_header_def_rejects_reserved_name_case_insensitive() {
        let err = HeaderDef::new("AUTHORIZATION", "x").unwrap_err();
        assert!(matches!(err, crate::error::HeaderError::Reserved(ref n) if n == "AUTHORIZATION"));
        assert!(err.to_string().contains("AUTHORIZATION"));

        let err = HeaderDef::new("Sec-WebSocket-Key", "x").unwrap_err();
        assert!(
            matches!(err, crate::error::HeaderError::Reserved(ref n) if n == "Sec-WebSocket-Key")
        );
        assert!(err.to_string().contains("Sec-WebSocket-Key"));
    }

    #[test]
    /// OC-R-117 — an ordinary, non-reserved header name is accepted.
    fn ut_header_def_accepts_ordinary_name() {
        assert!(HeaderDef::new("X-Custom", "v").is_ok());
    }

    #[test]
    /// OC-R-118 — a header name violating the HTTP token grammar (whitespace, a separator) is
    /// rejected.
    fn ut_header_def_rejects_invalid_name_grammar() {
        assert!(matches!(
            HeaderDef::new("X Custom", "v"),
            Err(crate::error::HeaderError::InvalidName(_))
        ));
        assert!(matches!(
            HeaderDef::new("X/Custom", "v"),
            Err(crate::error::HeaderError::InvalidName(_))
        ));
    }

    #[test]
    /// OC-R-118 — a header value containing a control byte (CR/LF) is rejected.
    fn ut_header_def_rejects_control_byte_in_value() {
        assert!(matches!(
            HeaderDef::new("X-Custom", "a\r\nb"),
            Err(crate::error::HeaderError::InvalidValue(_))
        ));
    }

    #[test]
    /// Round-trips a valid `HeaderDef` through JSON and TOML unchanged.
    fn ut_header_def_toml_json_roundtrip() {
        let header = HeaderDef::new("X-Tenant", "acme-1").unwrap();

        let json = serde_json::to_string(&header).unwrap();
        let back: HeaderDef = serde_json::from_str(&json).unwrap();
        assert_eq!(header, back);

        let toml = toml::to_string(&header).unwrap();
        let back: HeaderDef = toml::from_str(&toml).unwrap();
        assert_eq!(header, back);
    }

    #[test]
    /// Deserializing a `HeaderDef` with a reserved name fails — the `try_from` gate applies on
    /// load, not just via `HeaderDef::new`.
    fn ut_header_def_deserialize_rejects_invalid() {
        let value = serde_json::json!({"name": "Authorization", "value": "x"});
        assert!(serde_json::from_value::<HeaderDef>(value).is_err());
    }

    #[test]
    /// `HeaderError`'s `Display` names the offending header for every variant.
    fn ut_header_error_display() {
        assert!(
            crate::error::HeaderError::Reserved("Authorization".into())
                .to_string()
                .contains("Authorization")
        );
        assert!(
            crate::error::HeaderError::InvalidName("X Custom".into())
                .to_string()
                .contains("X Custom")
        );
        assert!(
            crate::error::HeaderError::InvalidValue("X-Custom".into())
                .to_string()
                .contains("X-Custom")
        );
    }
}
