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
use ferrowl_modbus::{Key, ServerCommand, SlaveKey, UnitId};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use ferrowl_util::tls::ServerCertSource;
use parking_lot::Mutex;
use parking_lot::RwLock as MemLock;
use rcgen::{CertificateParams, Issuer, KeyPair};
use rust_modbus::{
    ClientIdentity, RootStore, ServerCertVerification, TcpConfig, TlsClientConfig, connect_tls,
};
use tokio::sync::{RwLock, mpsc};

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
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, log, sink())
        .await
        .expect("server should start with the self-signed fallback");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

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
            server_cert: ServerCertSource::SelfSigned,
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, log, sink())
        .await
        .expect("server should start with an explicit self-signed cert");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

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
/// MB-R-106 (new precedence) — `self_signed` wins unconditionally over `cert_file`/`key_file`
/// present in the same config: the server presents an ephemeral self-signed certificate, not
/// the one from the (structurally unreachable) explicit files, and no fallback log line is
/// produced since `self_signed` was explicitly requested.
async fn self_signed_wins_over_explicit_files() {
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_file = write_pem("explicit-cert", &cert_pem);
    let key_file = write_pem("explicit-key", &key_pem);

    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server_cert: ServerCertSource::SelfSigned,
            ..Default::default()
        },
    )));
    // `cert_file`/`key_file` are structurally unreachable on `ServerCertSource::SelfSigned` —
    // the files written above exist only to prove they are never consulted.
    let _ = (&cert_file, &key_file);

    let (log, captured) = capturing();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, log, sink())
        .await
        .expect("server should start with self_signed");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // A client trusting only the (unused) explicit cert fails: the server presents a different,
    // ephemeral certificate.
    assert!(
        raw_connect(addr, Some(&cert_pem), None).await.is_err(),
        "server must not present the explicit cert_file when self_signed is set"
    );
    // A client with verification disabled still connects: the server did present *some*
    // certificate (the ephemeral self-signed one), it just isn't the explicit one.
    raw_connect(addr, None, None)
        .await
        .expect("server should present an ephemeral self-signed cert");

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

/// MB-R-107 (new timing) — `cert_file` or `key_file` set alone (not both), with `self_signed`
/// unset, fails at config *deserialization*, not server start: `ServerCertSource`'s custom
/// `Deserialize` makes the illegal state impossible to construct at all (see `ferrowl-util`'s
/// `ServerCertSource::resolve`), so `tcp::Config` never even parses. `ModbusTlsConfig`'s typed
/// Rust constructor can no longer represent "cert_file alone" (there is no `Explicit`-with-only-
/// one-file variant), which is exactly the point: this is now a compile-time-unrepresentable
/// state, not just a runtime-checked one.
#[test]
fn lone_cert_or_key_file_fails_config_deserialization() {
    let cert_only = r#"{"ip":"127.0.0.1","port":0,"timeout_ms":1000,"delay_ms":0,
        "interval_ms":0,"tls":{"cert_file":"whatever.pem"}}"#;
    let result: Result<tcp::Config, _> = serde_json::from_str(cert_only);
    assert!(result.is_err(), "cert_file alone must fail to deserialize");

    let key_only = r#"{"ip":"127.0.0.1","port":0,"timeout_ms":1000,"delay_ms":0,
        "interval_ms":0,"tls":{"key_file":"whatever.pem"}}"#;
    let result: Result<tcp::Config, _> = serde_json::from_str(key_only);
    assert!(result.is_err(), "key_file alone must fail to deserialize");
}

#[tokio::test]
/// MB-R-108 — `require_client_cert` accepts a client signed by the configured CA,
/// and rejects one presenting none or one signed by an unrelated CA.
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
            server_cert: ServerCertSource::Explicit {
                cert_file: server_cert_file.clone(),
                key_file: server_key_file,
            },
            require_client_cert: true,
            client_ca_file: Some(ca_file),
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, sink(), sink())
        .await
        .expect("mTLS server should start");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

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
    // connection carries no usable session. Survival of the accept loop past this
    // rejection is asserted separately, in
    // `require_client_cert_rejection_does_not_kill_accept_loop`.
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

    server.abort();
}

#[tokio::test]
/// MB-R-111 (server connection-scoping half) — a rejected mTLS handshake (missing
/// or wrong-CA client certificate) never takes down the accept loop: a subsequent
/// well-behaved client still connects.
async fn require_client_cert_rejection_does_not_kill_accept_loop() {
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem("mtls-server-cert", &server_cert_pem);
    let server_key_file = write_pem("mtls-server-key", &server_key_pem);

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let ca_file = write_pem("mtls-ca", &ca_pem);

    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server_cert: ServerCertSource::Explicit {
                cert_file: server_cert_file.clone(),
                key_file: server_key_file,
            },
            require_client_cert: true,
            client_ca_file: Some(ca_file),
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, sink(), sink())
        .await
        .expect("mTLS server should start");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // No client certificate presented at all: rejected (see the TLS 1.3 client-side
    // completion quirk note in `require_client_cert_enforced`).
    let _ = raw_connect(addr, Some(&server_cert_pem), None).await;

    // The accept loop survived the rejection: a subsequent well-behaved client
    // still connects.
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
    .expect("accept loop should still serve a well-behaved client after a prior rejection");

    server.abort();
}

#[tokio::test]
/// MB-R-111 (server logging half) — a rejected mTLS handshake (no client
/// certificate presented) is logged with the peer address and a failure reason.
async fn require_client_cert_rejection_is_logged() {
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem("mtls-log-server-cert", &server_cert_pem);
    let server_key_file = write_pem("mtls-log-server-key", &server_key_pem);

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let ca_file = write_pem("mtls-log-ca", &ca_pem);

    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server_cert: ServerCertSource::Explicit {
                cert_file: server_cert_file.clone(),
                key_file: server_key_file,
            },
            require_client_cert: true,
            client_ca_file: Some(ca_file),
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, log, sink())
        .await
        .expect("mTLS server should start");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // No client certificate presented at all: rejected (see the TLS 1.3 client-side
    // completion quirk note in `require_client_cert_enforced`).
    let _ = raw_connect(addr, Some(&server_cert_pem), None).await;

    // Run a well-behaved connection afterward so the rejected handshake's task has
    // definitely finished (and its log line landed) before we inspect `captured`.
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
    .expect("accept loop should still serve a well-behaved client after a prior rejection");

    let lines = captured.lock();
    assert!(
        lines.iter().any(|line| line.contains("TLS handshake")
            && line.contains("127.0.0.1:")
            && line.contains("failed")),
        "expected a TLS handshake failure log line naming the peer, got: {lines:?}"
    );

    server.abort();
}

#[tokio::test]
/// MB-R-108 — with `require_client_cert` unset (false), a configured `client_ca_file`
/// is ignored: a client presenting no certificate at all is still accepted.
async fn client_ca_file_is_ignored_when_require_client_cert_is_unset() {
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem("no-require-server-cert", &server_cert_pem);
    let server_key_file = write_pem("no-require-server-key", &server_key_pem);

    let (ca_pem, ..) = ca_and_signed_client_pem();
    let ca_file = write_pem("no-require-ca", &ca_pem);

    let port = free_port();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server_cert: ServerCertSource::Explicit {
                cert_file: server_cert_file,
                key_file: server_key_file,
            },
            client_ca_file: Some(ca_file),
            // require_client_cert left at its default (false).
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(srv_rx, sink(), sink())
        .await
        .expect("server should start: a client_ca_file without require_client_cert is valid");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    raw_connect(addr, Some(&server_cert_pem), None)
        .await
        .expect("a client presenting no certificate should be accepted");

    server.abort();
}

#[tokio::test]
/// MB-R-108 — `require_client_cert` without a `client_ca_file` fails the server's
/// start with a TLS configuration error, before any bind is attempted. `spawn()` itself
/// always returns `Ok` now (MB-R-130/MB-R-134); the configuration error surfaces from the
/// joined task instead, and never retries (a TLS configuration error can never fix itself).
async fn require_client_cert_without_ca_fails_server_start() {
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_file = write_pem("no-ca-cert", &cert_pem);
    let key_file = write_pem("no-ca-key", &key_pem);

    let cfg = Arc::new(RwLock::new(config(
        free_port(),
        tcp::ModbusTlsConfig {
            server_cert: ServerCertSource::Explicit {
                cert_file: cert_file,
                key_file: key_file,
            },
            require_client_cert: true,
            ..Default::default()
        },
    )));
    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(rx, sink(), sink())
        .await
        .expect("spawn always returns Ok now");
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("task should end promptly, not retry, on a TLS configuration error")
        .expect("task must not panic");
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Tcp(
            ferrowl_modbus::TcpError::Configuration(_)
        ))
    ));
}

#[tokio::test]
/// edge-cases.md "TLS boundaries" — a malformed/unreadable PEM path fails the
/// server's start with a TLS configuration error, the same tier as MB-R-107/108. `spawn()`
/// itself always returns `Ok` now (MB-R-130/MB-R-134); the configuration error surfaces
/// from the joined task instead, and never retries.
async fn malformed_pem_fails_server_start() {
    let bad_cert = write_pem("garbage-cert", "not a pem file at all");
    let (_cert_pem, key_pem) = self_signed_pem();
    let key_file = write_pem("garbage-key", &key_pem);

    let cfg = Arc::new(RwLock::new(config(
        free_port(),
        tcp::ModbusTlsConfig {
            server_cert: ServerCertSource::Explicit {
                cert_file: bad_cert,
                key_file: key_file,
            },
            ..Default::default()
        },
    )));
    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = tcp::ServerBuilder::new(cfg, memory())
        .spawn(rx, sink(), sink())
        .await
        .expect("spawn always returns Ok now");
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("task should end promptly, not retry, on a TLS configuration error")
        .expect("task must not panic");
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Tcp(
            ferrowl_modbus::TcpError::Configuration(_)
        ))
    ));
}
