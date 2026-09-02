//! End-to-end UDP transport tests. The server-side tests are driven by `rust_modbus`'s own raw
//! `Client`/`connect_udp` directly (mirroring its own `tests/client_udp.rs`); the client-side
//! tests below drive `ferrowl_modbus::udp::Client`/`ClientBuilder` against `udp::ServerBuilder`.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::udp;
use ferrowl_modbus::{
    Command, Error, FunctionCode, Key, Operation, ServerCommand, SlaveKey, TcpError, UnitId,
};
use ferrowl_store::{CellKind, CellType, Memory, Range};
use ferrowl_test_support::reserve_udp_port;
use parking_lot::RwLock as MemLock;
use rust_modbus::{
    Address as RmAddress, Client as RmClient, Quantity as RmQuantity, RegisterValue, UdpConfig,
    connect_udp,
};
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn key(slave_id: UnitId, kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey { slave_id, kind })
}

/// A no-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

fn config(port: u16) -> udp::Config {
    udp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
    }
}

/// Server memory seeded with holding registers for slave 1 and slave 0.
fn server_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(UnitId(1), RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(UnitId(1), RegKind::HoldingRegister),
        &CellType::Register,
        &Range::new(0, 4),
        &[10, 20, 30, 40],
    )
    .unwrap();
    mem.add_ranges(
        key(UnitId(0), RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(UnitId(0), RegKind::HoldingRegister),
        &CellType::Register,
        &Range::new(0, 4),
        &[1, 2, 3, 4],
    )
    .unwrap();
    Arc::new(MemLock::new(mem))
}

#[tokio::test]
/// MB-R-119 — a Udp server answers a request from a peer it never `accept`ed, straight off a
/// datagram, against the shared store, exactly as MB-R-057-065 already require for every other
/// transport.
async fn it_udp_server_answers_a_request() {
    let mem = server_mem();
    let port = reserve_udp_port().release();
    let cfg = config(port);
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(cfg.clone())), mem)
            .spawn(srv_rx, sink(), sink())
            .await
            .expect("server failed to start");
    sleep(Duration::from_millis(50)).await;

    let addr: SocketAddr = format!("{}:{}", cfg.ip, cfg.port).parse().unwrap();
    let transport = connect_udp(addr, UdpConfig::default())
        .await
        .expect("associates");
    let mut client: rust_modbus::UdpClient = RmClient::new(transport);
    let values = client
        .read_holding_registers(UnitId(1), RmAddress(0), RmQuantity(4))
        .await
        .expect("reads");
    assert_eq!(
        values,
        vec![10, 20, 30, 40]
            .into_iter()
            .map(RegisterValue)
            .collect::<Vec<_>>()
    );

    server.abort();
}

#[tokio::test]
/// MB-R-182 — slave id 0 on Udp is an ordinary slave id: a request addressed to it is answered
/// like any other (unlike RTU/RtuOverTcp's MB-R-103 no-response rule, which does not extend to
/// Udp — Udp carries MBAP/`Tcp` framing, whose `is_broadcast` is always false).
async fn it_udp_server_answers_slave_zero() {
    let mem = server_mem();
    let port = reserve_udp_port().release();
    let cfg = config(port);
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(cfg.clone())), mem)
            .spawn(srv_rx, sink(), sink())
            .await
            .expect("server failed to start");
    sleep(Duration::from_millis(50)).await;

    let addr: SocketAddr = format!("{}:{}", cfg.ip, cfg.port).parse().unwrap();
    let transport = connect_udp(addr, UdpConfig::default())
        .await
        .expect("associates");
    let mut client: rust_modbus::UdpClient = RmClient::new(transport);
    let values = client
        .read_holding_registers(UnitId(0), RmAddress(0), RmQuantity(4))
        .await
        .expect("slave id 0 is answered, not skipped");
    assert_eq!(
        values,
        vec![1, 2, 3, 4]
            .into_iter()
            .map(RegisterValue)
            .collect::<Vec<_>>()
    );

    server.abort();
}

/// Client memory with the same regions declared but no values (the client fills them from
/// reads); slave 1 and slave 0 both declared, since MB-R-181's slave-zero test needs slave 0.
fn client_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(UnitId(1), RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.add_ranges(
        key(UnitId(0), RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    Arc::new(MemLock::new(mem))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-117, MB-R-181 — a Udp client round-trips a read through `udp::Client`/`ClientCore`, driven
/// against `udp::ServerBuilder` — the same shape `tcp_loopback.rs`/`rtu_over_tcp_loopback.rs`
/// already prove for their transports, now for `Udp`; slave id 0 is ordinary for
/// Udp: an operation addressed to it is polled and answered exactly like slave 1, not skipped.
async fn it_udp_client_polls_server_and_executes_commands() {
    let port = reserve_udp_port().release();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, _bound_addr) =
        udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(config(port))), srv_mem.clone())
            .spawn(srv_rx, sink(), sink())
            .await
            .expect("server failed to start");

    let operations = Arc::new(RwLock::new(vec![
        Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: UnitId(0),
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 4),
        },
    ]));

    let (tx, rx) = mpsc::channel::<Command>(16);
    let (client, _connected) = udp::ClientBuilder::<SlaveKey>::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    sleep(Duration::from_millis(500)).await;

    {
        let g = cli_mem.read();
        assert_eq!(
            g.read(
                key(UnitId(1), RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
            vec![10, 20, 30, 40]
        );
        assert_eq!(
            g.read(
                key(UnitId(0), RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
            vec![1, 2, 3, 4]
        );
    }

    tx.send(Command::Terminate).await.unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("client did not terminate in time")
        .expect("client task panicked");
    assert!(joined.is_ok());

    server.abort();
}

#[tokio::test]
/// MB-R-179 — an `ip`/`port` pair that does not parse as a socket address fails the same way
/// TCP does (MB-R-069): `Error::Tcp(TcpError::Address(_))`.
async fn it_udp_client_bad_address_fails_like_tcp() {
    let mut bad = config(502);
    bad.ip = "not.an.ip.address".to_string();

    assert!(matches!(
        udp::Client::connect(&bad).await,
        Err(Error::Tcp(TcpError::Address(_)))
    ));
}
