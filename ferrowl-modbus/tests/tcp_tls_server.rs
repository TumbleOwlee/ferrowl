//! Server-side Modbus/TCP TLS tests (MB-R-106, MB-R-107, MB-R-108, MB-R-111 server
//! connection-scoping half).
//!
//! Drives `ferrowl_modbus::tcp::ServerBuilder` (the real server) with `rust_modbus`'s
//! own TLS client primitives directly, never `ferrowl_modbus::tcp::Client` (covered by
//! tcp_tls_client.rs).

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Key, ServerCommand, SlaveKey, UnitId};
use ferrowl_store::{CellKind, CellType, Memory, Range};
use ferrowl_test_support::{TempDirGuard, reserve_tcp_port, reserve_temp_dir};
use ferrowl_util::tls::{CertSource, CertVerification, ServerTlsPolicy};
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
        &CellKind::read_write(CellType::Register),
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

fn write_pem(dir: &TempDirGuard, label: &str, pem: &str) -> String {
    let path = dir.join(format!("{label}.pem"));
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
        tls,
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
/// MB-R-106, MB-R-166, OC-R-095 — with an `Ephemeral` identity (the variant standing for "no TLS
/// material configured"), the server falls back to an ephemeral self-signed certificate,
/// and logs that fallback.
async fn self_signed_fallback_is_used_and_logged() {
    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                identity: CertSource::Ephemeral {},
            },
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
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
/// MB-R-106, MB-R-166, OC-R-095 — an explicit `SelfSigned` identity uses a self-signed certificate
/// without logging the "no configuration" fallback line.
async fn explicit_self_signed_is_used_without_fallback_log() {
    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                identity: CertSource::SelfSigned {},
            },
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
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
        "expected no fallback log line when SelfSigned was explicitly requested, got: {:?}",
        captured.lock()
    );

    server.abort();
}

/// MB-R-107 — `CertSource::Files` naming only `cert_file` (or only `key_file`) fails at config
/// *deserialization*: the type's own required fields make the illegal state unrepresentable,
/// so `tcp::Config` never even parses.
#[test]
fn lone_cert_or_key_file_fails_config_deserialization() {
    let cert_only = r#"{"ip":"127.0.0.1","port":0,"timeout_ms":1000,"delay_ms":0,
        "interval_ms":0,"tls":{"server":{"mode":"tls","identity":{"source":"files","cert_file":"whatever.pem"}}}}"#;
    let result: Result<tcp::Config, _> = serde_json::from_str(cert_only);
    assert!(result.is_err(), "cert_file alone must fail to deserialize");

    let key_only = r#"{"ip":"127.0.0.1","port":0,"timeout_ms":1000,"delay_ms":0,
        "interval_ms":0,"tls":{"server":{"mode":"tls","identity":{"source":"files","key_file":"whatever.pem"}}}}"#;
    let result: Result<tcp::Config, _> = serde_json::from_str(key_only);
    assert!(result.is_err(), "key_file alone must fail to deserialize");
}

#[tokio::test]
/// MB-R-108, NF-R-048 — `Mutual`'s `CaFiles` verification accepts a client signed by the configured
/// CA, and rejects one presenting none or one signed by an unrelated CA.
async fn require_client_cert_enforced() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem(&dir, "mtls-server-cert", &server_cert_pem);
    let server_key_file = write_pem(&dir, "mtls-server-key", &server_key_pem);

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let ca_file = write_pem(&dir, "mtls-ca", &ca_pem);

    let (other_ca_pem, other_cert_pem, other_key_pem) = ca_and_signed_client_pem();
    let _ = other_ca_pem;

    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Mutual {
                identity: CertSource::Files {
                    cert_file: server_cert_file.clone(),
                    key_file: server_key_file,
                },
                verification: CertVerification::CaFiles {
                    ca_files: vec![ca_file],
                },
            },
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
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
    // or (TLS 1.3 client-side completion quirk — see `tcp_tls_client.rs`) appears to
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
/// MB-R-173 — server-role `Skip` still requires a client certificate be presented (a
/// handshake with none fails, same as `CaFiles`), but performs no chain/identity validation
/// against any root store: a client presenting a certificate signed by nobody the server
/// trusts (a bare self-signed cert, not chained to any configured CA — here there is no CA
/// configured at all) is still accepted.
async fn skip_verify_requires_a_cert_but_never_validates_its_chain() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem(&dir, "skip-verify-server-cert", &server_cert_pem);
    let server_key_file = write_pem(&dir, "skip-verify-server-key", &server_key_pem);

    // A client cert self-signed by nobody the server trusts (not chained to any CA at all).
    let (untrusted_cert_pem, untrusted_key_pem) = self_signed_pem();

    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Mutual {
                identity: CertSource::Files {
                    cert_file: server_cert_file,
                    key_file: server_key_file,
                },
                verification: CertVerification::Skip {},
            },
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
            .spawn(srv_rx, sink(), sink())
            .await
            .expect("skip-verify mTLS server should start");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // An untrusted, unrelated self-signed cert: accepted anyway (chain/identity never checked).
    let untrusted_chain = rust_modbus::load_pem_cert_chain(untrusted_cert_pem.as_bytes()).unwrap();
    let untrusted_key = rust_modbus::load_pem_private_key(untrusted_key_pem.as_bytes()).unwrap();
    raw_connect(
        addr,
        Some(&server_cert_pem),
        Some(ClientIdentity {
            cert_chain: untrusted_chain,
            key: untrusted_key,
        }),
    )
    .await
    .expect("Skip must accept a presented certificate with no chain validation");

    // No client certificate presented at all: still rejected (see the TLS 1.3 client-side
    // completion quirk note in `require_client_cert_enforced` — the connect future can appear to
    // succeed to the client even though the server has already rejected it).
    let _ = raw_connect(addr, Some(&server_cert_pem), None).await;

    server.abort();
}

#[tokio::test]
/// MB-R-177 (server connection-scoping half) — a rejected mTLS handshake (missing
/// or wrong-CA client certificate) never takes down the accept loop: a subsequent
/// well-behaved client still connects.
async fn require_client_cert_rejection_does_not_kill_accept_loop() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem(&dir, "mtls-server-cert", &server_cert_pem);
    let server_key_file = write_pem(&dir, "mtls-server-key", &server_key_pem);

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let ca_file = write_pem(&dir, "mtls-ca", &ca_pem);

    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Mutual {
                identity: CertSource::Files {
                    cert_file: server_cert_file.clone(),
                    key_file: server_key_file,
                },
                verification: CertVerification::CaFiles {
                    ca_files: vec![ca_file],
                },
            },
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
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
/// MB-R-178 (server logging half) — a rejected mTLS handshake (no client
/// certificate presented) is logged with the peer address and a failure reason.
async fn require_client_cert_rejection_is_logged() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem(&dir, "mtls-log-server-cert", &server_cert_pem);
    let server_key_file = write_pem(&dir, "mtls-log-server-key", &server_key_pem);

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let ca_file = write_pem(&dir, "mtls-log-ca", &ca_pem);

    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Mutual {
                identity: CertSource::Files {
                    cert_file: server_cert_file.clone(),
                    key_file: server_key_file,
                },
                verification: CertVerification::CaFiles {
                    ca_files: vec![ca_file],
                },
            },
            ..Default::default()
        },
    )));
    let (log, captured) = capturing();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
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
/// MB-R-174 — `Tls` never requests a client certificate at all: a client presenting none is
/// still accepted, regardless of what verification `Mutual` would otherwise apply.
async fn it_tls_policy_never_requests_a_client_certificate() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem(&dir, "no-mtls-server-cert", &server_cert_pem);
    let server_key_file = write_pem(&dir, "no-mtls-server-key", &server_key_pem);

    let port = reserve_tcp_port().release();
    let cfg = Arc::new(RwLock::new(config(
        port,
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                identity: CertSource::Files {
                    cert_file: server_cert_file,
                    key_file: server_key_file,
                },
            },
            ..Default::default()
        },
    )));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
            .spawn(srv_rx, sink(), sink())
            .await
            .expect("server should start: Tls never requests a client certificate");

    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before a raw connect that,
    // unlike the ferrowl client, never retries.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    raw_connect(addr, Some(&server_cert_pem), None)
        .await
        .expect("a client presenting no certificate should be accepted under Tls");

    server.abort();
}

#[tokio::test]
/// MB-R-108 — `Mutual`'s `CaFiles { ca_files: [] }` is rejected by `validate()` before the
/// server ever binds, surfacing as the same TLS-configuration-error tier as a malformed PEM.
async fn it_empty_ca_files_fails_server_start() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_file = write_pem(&dir, "empty-ca-server-cert", &cert_pem);
    let key_file = write_pem(&dir, "empty-ca-server-key", &key_pem);

    let cfg = Arc::new(RwLock::new(config(
        reserve_tcp_port().release(),
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Mutual {
                identity: CertSource::Files {
                    cert_file,
                    key_file,
                },
                verification: CertVerification::CaFiles { ca_files: vec![] },
            },
            ..Default::default()
        },
    )));
    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
            .spawn(rx, sink(), sink())
            .await
            .expect("spawn always returns Ok");
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("task should end promptly on a rejected policy, not hang")
        .expect("task must not panic");
    match result {
        Err(ferrowl_modbus::Error::Tcp(ferrowl_modbus::TcpError::Configuration(msg))) => {
            assert!(
                msg.contains("ca_files"),
                "expected the empty-ca_files rejection message, got: {msg}"
            );
        }
        other => panic!("expected TcpError::Configuration naming ca_files, got {other:?}"),
    }
}

#[tokio::test]
/// edge-cases.md "TLS boundaries" — a malformed/unreadable PEM path fails the
/// server's start with a TLS configuration error, the same tier as MB-R-107/108. `spawn()`
/// itself always returns `Ok` now (MB-R-130/MB-R-134); the configuration error surfaces
/// from the joined task instead, and never retries.
async fn malformed_pem_fails_server_start() {
    let dir = reserve_temp_dir("ferrowl_modbus_tcp_tls_server");
    let bad_cert = write_pem(&dir, "garbage-cert", "not a pem file at all");
    let (_cert_pem, key_pem) = self_signed_pem();
    let key_file = write_pem(&dir, "garbage-key", &key_pem);

    let cfg = Arc::new(RwLock::new(config(
        reserve_tcp_port().release(),
        tcp::ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                identity: CertSource::Files {
                    cert_file: bad_cert,
                    key_file,
                },
            },
            ..Default::default()
        },
    )));
    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) =
        tcp::ServerBuilder::new(cfg, memory(), tcp::new_self_signed_cache())
            .spawn(rx, sink(), sink())
            .await
            .expect("spawn always returns Ok");
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
