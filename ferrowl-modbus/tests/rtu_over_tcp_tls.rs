//! RTU-over-TCP TLS tests (MB-R-115): the same TLS glue plain TCP uses (MB-R-104..MB-R-111),
//! reused verbatim under RTU framing.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::rtu_over_tcp;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Command, FunctionCode, Key, Operation, ServerCommand, SlaveKey, UnitId};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use parking_lot::RwLock as MemLock;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn key(kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey {
        slave_id: UnitId(1),
        kind,
    })
}

fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn server_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(RegKind::HoldingRegister),
        &CellType::Register,
        &Range::new(0, 4),
        &[10, 20, 30, 40],
    )
    .unwrap();
    Arc::new(MemLock::new(mem))
}

fn client_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    Arc::new(MemLock::new(mem))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-115 — an RTU-over-TCP client and server, both configured for TLS, complete the
/// handshake and exchange Modbus RTU-framed traffic over it, exactly as plain TCP does.
async fn rtu_over_tcp_client_server_tls_roundtrip() {
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    // Server: an ephemeral self-signed cert (`self_signed = true`, no cert_file/key_file).
    let server_tls = tcp::ModbusTlsConfig {
        self_signed: true,
        ..Default::default()
    };
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let server =
        rtu_over_tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port, server_tls))), srv_mem)
            .spawn(srv_rx, sink(), sink())
            .await
            .expect("server failed to start");
    // `spawn()` only guarantees the task was scheduled, not that its first bind/TLS-config
    // attempt has run yet (MB-R-130/MB-R-134); give it a moment before starting the client so
    // its first connect attempt does not race the server into a full backoff wait.
    sleep(std::time::Duration::from_millis(100)).await;

    // Client: trusts whatever certificate the server presents (this test is about the
    // RTU-over-TCP handshake plumbing, not certificate validation, which MB-R-109's
    // tcp_tls_client.rs tests already cover in depth).
    let client_tls = tcp::ModbusTlsConfig {
        insecure_skip_verify: true,
        ..Default::default()
    };
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: UnitId(1),
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = rtu_over_tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port, client_tls))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    sleep(std::time::Duration::from_millis(800)).await;

    {
        let g = cli_mem.read();
        assert_eq!(
            g.read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
            vec![10, 20, 30, 40]
        );
    }

    tx.send(Command::Terminate).await.unwrap();
    let joined = tokio::time::timeout(std::time::Duration::from_secs(5), client)
        .await
        .expect("client did not terminate in time")
        .expect("client task panicked");
    assert!(joined.is_ok());
    server.abort();
}

#[tokio::test]
/// MB-R-115 — a TLS-configured RTU-over-TCP client against a plain (non-TLS) listener fails the
/// connect with a handshake error, same as plain TCP (MB-R-111).
async fn rtu_over_tcp_tls_handshake_failure_is_connect_failure() {
    let plain_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let plain_addr = plain_listener.local_addr().unwrap();
    let plain_server = tokio::spawn(async move {
        loop {
            if plain_listener.accept().await.is_err() {
                break;
            }
        }
    });

    let handshake_cfg = config(
        plain_addr.port(),
        tcp::ModbusTlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        },
    );
    let result = rtu_over_tcp::Client::connect(&handshake_cfg).await;
    assert!(
        result.is_err(),
        "expected TLS handshake against a plain listener to fail"
    );
    plain_server.abort();
}
