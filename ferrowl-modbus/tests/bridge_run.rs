//! bridge::run wiring tests (stage s9). Only the tcp-upstream/tcp-downstream combination
//! runs a real end-to-end path through `bridge::run` itself here: both transports work
//! without hardware in a test environment. The other three combinations involve RTU, whose
//! downstream/upstream types are hard-pinned to a real `SerialStream` (no hardware available
//! in CI, matching the existing `rtu_serial.rs` convention of only exercising the
//! open-failure path) — those combinations' service/type wiring is proven instead at the
//! layer below `bridge::run` (`BridgeService`, `upstream_tcp::run`, `upstream_rtu::run`) by
//! crate-internal tests alongside `bridge::run`'s own definition, per BR-R-002/005/006.

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::bridge::{BridgeConfig, BridgeEndpointKind, BridgeEndpointSpec};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use parking_lot::RwLock as MemLock;
use rust_modbus::{Address, Client as RmClient, FrameTransport, Quantity, RegisterValue, UnitId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock as TokioRwLock;

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

fn tcp_config(port: u16) -> ferrowl_modbus::tcp::Config {
    ferrowl_modbus::tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
        tls: None,
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
    let downstream_port = free_port();
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &MemKind::ReadWrite(CellType::Register),
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
    let _downstream_server = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(TokioRwLock::new(tcp_config(downstream_port))),
        srv_mem,
    )
    .spawn(sink())
    .await
    .expect("downstream server failed to start");

    let upstream_port = free_port();
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
