//! bridge::run wiring tests. Only the tcp-upstream/tcp-downstream combination
//! runs a real end-to-end path through `bridge::run` itself here: both transports work
//! without hardware in a test environment. The other three combinations involve RTU, whose
//! downstream/upstream types are hard-pinned to a real `SerialStream` (no hardware available
//! in CI, matching the existing `rtu_serial.rs` convention of only exercising the
//! open-failure path) — those combinations' service/type wiring is proven instead at the
//! layer below `bridge::run` (`BridgeService`, `upstream_tcp::run`, `upstream_rtu::run`) by
//! crate-internal tests alongside `bridge::run`'s own definition, per BR-R-002/005/006.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::bridge::{BridgeConfig, BridgeEndpointKind, BridgeEndpointSpec};
use ferrowl_modbus::{Key, ServerCommand, SlaveKey};
use ferrowl_store::{CellKind, CellType, Memory, Range};
use parking_lot::RwLock as MemLock;
use rust_modbus::{Address, Client as RmClient, FrameTransport, Quantity, RegisterValue, UnitId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::mpsc;

fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

/// Polls a `ServerBuilder::spawn`-returned `BoundAddr` until the listener actually binds,
/// instead of racing it with a fixed sleep (MB-R-130 companion — `spawn()` only guarantees the
/// task was scheduled, not that its first bind attempt has run).
async fn wait_bound_addr(bound_addr: &Arc<parking_lot::Mutex<Option<std::net::SocketAddr>>>) {
    for _ in 0..50 {
        if bound_addr.lock().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("listener did not bind within 1s");
}

fn tcp_config(port: u16) -> ferrowl_modbus::tcp::Config {
    ferrowl_modbus::tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
        tls: Default::default(),
    }
}

fn key(kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey {
        slave_id: UnitId(1),
        kind,
    })
}

/// BR-R-002, BR-R-005, BR-R-006 — `bridge::run` wired tcp-upstream/tcp-downstream: the
/// upstream side accepts (server role), the downstream side connects (client role), and a
/// request issued to the upstream port is relayed through to the seeded downstream value
/// unmodified. No TUI/session/store of its own is constructed anywhere in this path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_bridge_run_wires_tcp_upstream_tcp_downstream() {
    let downstream_port = ferrowl_test_support::reserve_tcp_port().release();
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(RegKind::HoldingRegister),
        &CellType::Register,
        &Range::new(0, 1),
        &[42],
    )
    .unwrap();
    let srv_mem = Arc::new(MemLock::new(mem));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (_downstream_server, bound_addr) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(TokioRwLock::new(tcp_config(downstream_port))),
        srv_mem,
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(srv_rx, sink(), sink())
    .await
    .expect("downstream server failed to start");
    wait_bound_addr(&bound_addr).await;

    let upstream_port = ferrowl_test_support::reserve_tcp_port().release();
    let config = BridgeConfig {
        upstream: BridgeEndpointSpec {
            kind: BridgeEndpointKind::Tcp(tcp_config(upstream_port)),
            unit_ids: None,
        },
        downstream: BridgeEndpointSpec {
            kind: BridgeEndpointKind::Tcp(tcp_config(downstream_port)),
            unit_ids: None,
        },
    };
    let _bridge = ferrowl_modbus::bridge::run(config, sink())
        .await
        .expect("bridge failed to start");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client: RmClient<_, rust_modbus::Tcp> = RmClient::new(FrameTransport::new(
        tokio::net::TcpStream::connect(("127.0.0.1", upstream_port))
            .await
            .expect("client connects to upstream"),
    ));
    let registers = client
        .read_holding_registers(UnitId(1), Address(0), Quantity(1))
        .await
        .unwrap();
    assert_eq!(registers, vec![RegisterValue(42)]);
}

/// BR-R-002, BR-R-004, BR-R-005, BR-R-006 — `bridge::run` wired rtu_over_tcp-upstream/
/// rtu_over_tcp-downstream: like the plain-TCP case, both sides are TCP sockets (RTU framing
/// instead of MBAP), so this combination also runs a real end-to-end path with no hardware.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_bridge_run_wires_rtu_over_tcp_upstream_rtu_over_tcp_downstream() {
    let downstream_port = ferrowl_test_support::reserve_tcp_port().release();
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(RegKind::HoldingRegister),
        &CellType::Register,
        &Range::new(0, 1),
        &[43],
    )
    .unwrap();
    let srv_mem = Arc::new(MemLock::new(mem));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (_downstream_server, bound_addr) = ferrowl_modbus::rtu_over_tcp::ServerBuilder::new(
        Arc::new(TokioRwLock::new(tcp_config(downstream_port))),
        srv_mem,
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(srv_rx, sink(), sink())
    .await
    .expect("downstream server failed to start");
    wait_bound_addr(&bound_addr).await;

    let upstream_port = ferrowl_test_support::reserve_tcp_port().release();
    let config = BridgeConfig {
        upstream: BridgeEndpointSpec {
            kind: BridgeEndpointKind::RtuOverTcp(tcp_config(upstream_port)),
            unit_ids: None,
        },
        downstream: BridgeEndpointSpec {
            kind: BridgeEndpointKind::RtuOverTcp(tcp_config(downstream_port)),
            unit_ids: None,
        },
    };
    let _bridge = ferrowl_modbus::bridge::run(config, sink())
        .await
        .expect("bridge failed to start");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client: RmClient<_, rust_modbus::RtuOverTcp> = RmClient::new(FrameTransport::new(
        tokio::net::TcpStream::connect(("127.0.0.1", upstream_port))
            .await
            .expect("client connects to upstream"),
    ));
    let registers = client
        .read_holding_registers(UnitId(1), Address(0), Quantity(1))
        .await
        .unwrap();
    assert_eq!(registers, vec![RegisterValue(43)]);
}

/// BR-R-002, BR-R-004, BR-R-005, BR-R-006 — `bridge::run` wired ascii_over_tcp-upstream/
/// ascii_over_tcp-downstream: like the RTU-over-TCP case, a real end-to-end path with no
/// hardware, this time carrying Modbus ASCII framing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_bridge_run_wires_ascii_over_tcp_upstream_ascii_over_tcp_downstream() {
    let downstream_port = ferrowl_test_support::reserve_tcp_port().release();
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(RegKind::HoldingRegister),
        &CellType::Register,
        &Range::new(0, 1),
        &[44],
    )
    .unwrap();
    let srv_mem = Arc::new(MemLock::new(mem));
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (_downstream_server, bound_addr) = ferrowl_modbus::ascii_over_tcp::ServerBuilder::new(
        Arc::new(TokioRwLock::new(tcp_config(downstream_port))),
        srv_mem,
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(srv_rx, sink(), sink())
    .await
    .expect("downstream server failed to start");
    wait_bound_addr(&bound_addr).await;

    let upstream_port = ferrowl_test_support::reserve_tcp_port().release();
    let config = BridgeConfig {
        upstream: BridgeEndpointSpec {
            kind: BridgeEndpointKind::AsciiOverTcp(tcp_config(upstream_port)),
            unit_ids: None,
        },
        downstream: BridgeEndpointSpec {
            kind: BridgeEndpointKind::AsciiOverTcp(tcp_config(downstream_port)),
            unit_ids: None,
        },
    };
    let _bridge = ferrowl_modbus::bridge::run(config, sink())
        .await
        .expect("bridge failed to start");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client: RmClient<_, rust_modbus::Ascii> = RmClient::new(FrameTransport::new(
        tokio::net::TcpStream::connect(("127.0.0.1", upstream_port))
            .await
            .expect("client connects to upstream"),
    ));
    let registers = client
        .read_holding_registers(UnitId(1), Address(0), Quantity(1))
        .await
        .unwrap();
    assert_eq!(registers, vec![RegisterValue(44)]);
}
