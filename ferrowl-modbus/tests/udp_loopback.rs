//! End-to-end UDP transport tests. This stage (s2) covers the server half only, driven by
//! `rust_modbus`'s own raw `Client`/`connect_udp` directly (mirroring its own
//! `tests/client_udp.rs`) — `ferrowl_modbus::udp::Client` is added in a later stage.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::udp;
use ferrowl_modbus::{Key, SlaveKey, UnitId};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use parking_lot::RwLock as MemLock;
use rust_modbus::{
    Address as RmAddress, Client as RmClient, Quantity as RmQuantity, RegisterValue, UdpConfig,
    connect_udp,
};
use tokio::sync::RwLock;
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn key(slave_id: UnitId, kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey { slave_id, kind })
}

/// A no-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

/// An OS-assigned free UDP port (bind to :0, read the port, drop the socket).
async fn free_udp_port() -> u16 {
    tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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
        &MemKind::ReadWrite(CellType::Register),
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
        &MemKind::ReadWrite(CellType::Register),
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
    let port = free_udp_port().await;
    let cfg = config(port);
    let server = udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(cfg.clone())), mem)
        .spawn(sink())
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
/// MB-R-119 — slave id 0 on Udp is an ordinary slave id: a request addressed to it is answered
/// like any other (unlike RTU/RtuOverTcp's MB-R-103 no-response rule, which does not extend to
/// Udp — Udp carries MBAP/`Tcp` framing, whose `is_broadcast` is always false).
async fn it_udp_server_answers_slave_zero() {
    let mem = server_mem();
    let port = free_udp_port().await;
    let cfg = config(port);
    let server = udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(cfg.clone())), mem)
        .spawn(sink())
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

#[tokio::test]
/// MB-R-120 — a bind failure (port already occupied) surfaces as an error from `spawn`, is not
/// retried, mirroring `tcp::server`'s own bind-failure test.
async fn it_udp_server_bind_failure_surfaces() {
    let port = free_udp_port().await;
    let _occupier = tokio::net::UdpSocket::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let mem: Mem = Arc::new(MemLock::new(Memory::default()));
    let res = udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(config(port))), mem)
        .spawn(sink())
        .await;
    assert!(res.is_err());
}
