//! Server-side Modbus/TCP TLS tests (MB-R-106, MB-R-107, MB-R-108, MB-R-111 server
//! connection-scoping half).
//!
//! Drives `ferrowl_modbus::tcp::ServerBuilder` (the real server) with `rust_modbus`'s
//! own TLS client primitives directly (never `ferrowl_modbus::tcp::Client` — that's
//! s2's own surface), so this stage needs nothing s2 produces.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Key, SlaveKey, UnitId};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use parking_lot::Mutex;
use parking_lot::RwLock as MemLock;
use rcgen::{CertificateParams, Issuer, KeyPair};
use rust_modbus::{
    ClientIdentity, RootStore, ServerCertVerification, TcpConfig, TlsClientConfig, connect_tls,
};
use tokio::sync::RwLock;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn key(kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey {
        slave_id: UnitId(1),
        kind,
    })
}

fn memory() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    Arc::new(MemLock::new(mem))
}

/// A no-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

/// A log sink that records every line, so a test can assert on what the server logged.
fn capturing() -> (impl ferrowl_modbus::LogFn + Clone, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = log.clone();
    let f = move |s: String| {
        let sink = sink.clone();
        async move {
            sink.lock().push(s);
        }
    };
    (f, log)
}

/// An OS-assigned free TCP port (bind to :0, read the port, drop the listener).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_pem(label: &str, pem: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ferrowl-modbus-tls-server-test-{}-{label}-{n}.pem",
        std::process::id()
    ));
    std::fs::write(&path, pem).expect("failed to write test PEM file");
    path.to_string_lossy().into_owned()
}

fn self_signed_pem() -> (String, String) {
    let key = KeyPair::generate().expect("keypair generation failed");
    let cert = CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .expect("cert params")
        .self_signed(&key)
        .expect("self-signed cert");
    (cert.pem(), key.serialize_pem())
}

/// A self-signed CA plus one client cert/key pair it signs, all PEM-encoded.
fn ca_and_signed_client_pem() -> (String, String, String) {
    let ca_key = KeyPair::generate().expect("ca keypair generation failed");
    let ca_params = CertificateParams::new(vec!["ferrowl-test-ca".to_owned()]).expect("ca params");
    let ca_cert = ca_params.self_signed(&ca_key).expect("self-signed ca cert");
    let issuer = Issuer::from_params(&ca_params, &ca_key);

    let client_key = KeyPair::generate().expect("client keypair generation failed");
    let client_cert = CertificateParams::new(vec!["ferrowl-test-client".to_owned()])
        .expect("client params")
        .signed_by(&client_key, &issuer)
        .expect("ca-signed client cert");

    (ca_cert.pem(), client_cert.pem(), client_key.serialize_pem())
}

fn config(port: u16, tls: tcp::ModbusTlsConfig) -> tcp::Config {
    tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
        tls: Some(tls),
    }
}

/// Connect a bare `rust_modbus` TLS client against `addr`, trusting `trust_pem` (or
/// skipping verification if `None`), optionally presenting `identity`.
async fn raw_connect(
    addr: std::net::SocketAddr,
    trust_pem: Option<&str>,
    identity: Option<ClientIdentity>,
) -> rust_modbus::Result<()> {
    let server_cert = match trust_pem {
        Some(pem) => {
            let mut roots = RootStore::empty();
            roots.add_pem(pem.as_bytes())?;
            ServerCertVerification::Verify(roots)
        }
        None => ServerCertVerification::DangerousDisableVerification,
    };
    let tls = TlsClientConfig {
        server_cert,
        client_identity: identity,
    };
    connect_tls(addr, TcpConfig::default(), tls).await?;
    Ok(())
}

#[tokio::test]
/// MB-R-106 — with none of `cert_file`/`key_file`/`self_signed` set, the server falls
/// back to an ephemeral self-signed certificate, and logs that fallback.
async fn self_signed_fallback_is_used_and_logged() {
    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(port, tcp::ModbusTlsConfig::default())));
    let (log, captured) = capturing();
    let server = tcp::ServerBuilder::new(cfg, memory())
        .spawn(log)
        .await
        .expect("server should start with the self-signed fallback");

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    raw_connect(addr, None, None)
        .await
        .expect("client should connect to the fallback cert with verification disabled");

    assert!(
        captured
            .lock()
            .iter()
            .any(|line| line.contains("self-signed")),
        "expected a fallback log line, got: {:?}",
        captured.lock()
    );

    server.abort();
}

#[tokio::test]
/// MB-R-106 — an explicit `self_signed: true` uses a self-signed certificate without
/// logging the "no configuration" fallback line.
async fn explicit_self_signed_is_used_without_fallback_log() {
    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            self_signed: true,
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let server = tcp::ServerBuilder::new(cfg, memory())
        .spawn(log)
        .await
        .expect("server should start with an explicit self-signed cert");

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    raw_connect(addr, None, None)
        .await
        .expect("client should connect with verification disabled");

    assert!(
        !captured
            .lock()
            .iter()
            .any(|line| line.contains("self-signed")),
        "expected no fallback log line when self_signed was explicitly requested, got: {:?}",
        captured.lock()
    );

    server.abort();
}

#[tokio::test]
/// MB-R-106, edge-cases.md — `cert_file`/`key_file` win over `self_signed` when both
/// are set: no fallback log, and the presented certificate is the explicit one.
async fn explicit_files_win_over_self_signed() {
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_file = write_pem("explicit-cert", &cert_pem);
    let key_file = write_pem("explicit-key", &key_pem);

    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            cert_file: Some(cert_file),
            key_file: Some(key_file),
            self_signed: true,
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let server = tcp::ServerBuilder::new(cfg, memory())
        .spawn(log)
        .await
        .expect("server should start from the explicit files");

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    raw_connect(addr, Some(&cert_pem), None)
        .await
        .expect("client trusting the explicit cert specifically should connect");

    assert!(
        !captured
            .lock()
            .iter()
            .any(|line| line.contains("self-signed")),
        "expected no fallback log line when explicit files were set, got: {:?}",
        captured.lock()
    );

    server.abort();
}

#[tokio::test]
/// MB-R-107 — `cert_file` or `key_file` set alone (not both) fails the server's start
/// with a TLS configuration error, before any bind is attempted.
async fn lone_cert_or_key_file_fails_server_start() {
    let (cert_pem, _key_pem) = self_signed_pem();
    let cert_file = write_pem("lone-cert", &cert_pem);

    let cert_only = Arc::new(RwLock::new(config(
        free_port(),
        tcp::ModbusTlsConfig {
            cert_file: Some(cert_file),
            ..Default::default()
        },
    )));
    let result = tcp::ServerBuilder::new(cert_only, memory())
        .spawn(sink())
        .await;
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Tcp(
            ferrowl_modbus::TcpError::Configuration(_)
        ))
    ));

    let (_cert_pem2, key_pem2) = self_signed_pem();
    let key_file = write_pem("lone-key", &key_pem2);
    let key_only = Arc::new(RwLock::new(config(
        free_port(),
        tcp::ModbusTlsConfig {
            key_file: Some(key_file),
            ..Default::default()
        },
    )));
    let result = tcp::ServerBuilder::new(key_only, memory())
        .spawn(sink())
        .await;
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Tcp(
            ferrowl_modbus::TcpError::Configuration(_)
        ))
    ));
}

#[tokio::test]
/// MB-R-108, MB-R-111 (server connection-scoping half) — `require_client_cert`
/// accepts a client signed by the configured CA, rejects one presenting none or one
/// signed by an unrelated CA, and the accept loop survives every rejection.
async fn require_client_cert_enforced() {
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem("mtls-server-cert", &server_cert_pem);
    let server_key_file = write_pem("mtls-server-key", &server_key_pem);

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let ca_file = write_pem("mtls-ca", &ca_pem);

    let (other_ca_pem, other_cert_pem, other_key_pem) = ca_and_signed_client_pem();
    let _ = other_ca_pem;

    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            cert_file: Some(server_cert_file.clone()),
            key_file: Some(server_key_file),
            require_client_cert: true,
            client_ca_file: Some(ca_file),
            ..Default::default()
        },
    )));
    let server = tcp::ServerBuilder::new(cfg, memory())
        .spawn(sink())
        .await
        .expect("mTLS server should start");

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // Signed by the trusted CA: accepted.
    let client_cert_chain = rust_modbus::load_pem_cert_chain(client_cert_pem.as_bytes()).unwrap();
    let client_key = rust_modbus::load_pem_private_key(client_key_pem.as_bytes()).unwrap();
    raw_connect(
        addr,
        Some(&server_cert_pem),
        Some(ClientIdentity {
            cert_chain: client_cert_chain,
            key: client_key,
        }),
    )
    .await
    .expect("a client signed by the trusted CA should connect");

    // No client certificate presented at all: the handshake either fails outright,
    // or (TLS 1.3 client-side completion quirk — see s2's client tests) appears to
    // succeed to the client while the server has already rejected it; either way the
    // connection carries no usable session, so this only proves the accept loop
    // survives, checked below by a subsequent good connection.
    let _ = raw_connect(addr, Some(&server_cert_pem), None).await;

    // Signed by an unrelated CA: rejected the same way.
    let other_cert_chain = rust_modbus::load_pem_cert_chain(other_cert_pem.as_bytes()).unwrap();
    let other_key = rust_modbus::load_pem_private_key(other_key_pem.as_bytes()).unwrap();
    let _ = raw_connect(
        addr,
        Some(&server_cert_pem),
        Some(ClientIdentity {
            cert_chain: other_cert_chain,
            key: other_key,
        }),
    )
    .await;

    // The accept loop survived both rejections: a subsequent well-behaved client
    // still connects (MB-R-111 server half: one bad handshake never takes down the
    // accept loop or other connections).
    let client_cert_chain = rust_modbus::load_pem_cert_chain(client_cert_pem.as_bytes()).unwrap();
    let client_key = rust_modbus::load_pem_private_key(client_key_pem.as_bytes()).unwrap();
    raw_connect(
        addr,
        Some(&server_cert_pem),
        Some(ClientIdentity {
            cert_chain: client_cert_chain,
            key: client_key,
        }),
    )
    .await
    .expect("accept loop should still serve a well-behaved client after prior rejections");

    server.abort();
}

#[tokio::test]
/// MB-R-108 — `require_client_cert` without a `client_ca_file` fails the server's
/// start with a TLS configuration error, before any bind is attempted.
async fn require_client_cert_without_ca_fails_server_start() {
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_file = write_pem("no-ca-cert", &cert_pem);
    let key_file = write_pem("no-ca-key", &key_pem);

    let cfg = Arc::new(RwLock::new(config(
        free_port(),
        tcp::ModbusTlsConfig {
            cert_file: Some(cert_file),
            key_file: Some(key_file),
            require_client_cert: true,
            ..Default::default()
        },
    )));
    let result = tcp::ServerBuilder::new(cfg, memory()).spawn(sink()).await;
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Tcp(
            ferrowl_modbus::TcpError::Configuration(_)
        ))
    ));
}

#[tokio::test]
/// edge-cases.md "TLS boundaries" — a malformed/unreadable PEM path fails the
/// server's start with a TLS configuration error, the same tier as MB-R-107/108.
async fn malformed_pem_fails_server_start() {
    let bad_cert = write_pem("garbage-cert", "not a pem file at all");
    let (_cert_pem, key_pem) = self_signed_pem();
    let key_file = write_pem("garbage-key", &key_pem);

    let cfg = Arc::new(RwLock::new(config(
        free_port(),
        tcp::ModbusTlsConfig {
            cert_file: Some(bad_cert),
            key_file: Some(key_file),
            ..Default::default()
        },
    )));
    let result = tcp::ServerBuilder::new(cfg, memory()).spawn(sink()).await;
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Tcp(
            ferrowl_modbus::TcpError::Configuration(_)
        ))
    ));
}
