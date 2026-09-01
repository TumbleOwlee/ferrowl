//! End-to-end TCP downstream test: `bridge::spawn_tcp_downstream` against a real ferrowl
//! Modbus TCP server, and against a socket that accepts but never answers.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::bridge;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Address, Key, ServerCommand, SlaveKey, UnitId};
use ferrowl_store::{CellKind, CellType, Memory, Range};
use parking_lot::RwLock as MemLock;
use rust_modbus::{ExceptionCode, Quantity, RequestPdu};
use tokio::sync::{RwLock, mpsc};

fn key(kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey {
        slave_id: UnitId(1),
        kind,
    })
}

/// A no-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
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
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("listener did not bind within 1s");
}

fn config(port: u16) -> tcp::Config {
    tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 200,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
        tls: Default::default(),
    }
}

/// BR-R-006, BR-R-007 — a `TcpDownstream` connects to a real ferrowl Modbus TCP server and
/// forwards a request/response pair unmodified.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_tcp_downstream_connects_and_forwards() {
    let port = ferrowl_test_support::reserve_tcp_port().release();

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
    let (_server, bound_addr) = tcp::ServerBuilder::new(
        Arc::new(RwLock::new(config(port))),
        srv_mem,
        tcp::new_self_signed_cache(),
    )
    .spawn(srv_rx, sink(), sink())
    .await
    .expect("server failed to start");
    wait_bound_addr(&bound_addr).await;

    let downstream = bridge::spawn_tcp_downstream(config(port), sink());
    // Give the downstream's background reconnector time to connect.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let result = downstream
        .forward(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(1),
            },
        )
        .await
        .expect("forward succeeds");

    assert_eq!(
        result,
        Some(rust_modbus::ResponsePdu::ReadHoldingRegisters {
            registers: vec![rust_modbus::RegisterValue(42)],
        })
    );
}

/// BR-R-010 — a downstream whose TCP connect succeeds but whose Modbus exchange never gets a
/// response times out and reports `GatewayTargetDeviceFailedToRespond`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_tcp_downstream_connect_refused_then_reconnects_once_listener_appears() {
    let guard = ferrowl_test_support::reserve_tcp_port();
    let port = guard.port();
    // Bound but never accept()ed: the TCP connect succeeds instantly, nothing ever answers
    // the Modbus exchange.
    std::mem::forget(guard.into_listener());

    let downstream = bridge::spawn_tcp_downstream(config(port), sink());
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let result = downstream
        .forward(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(1),
            },
        )
        .await;

    assert_eq!(
        result,
        Err(ExceptionCode::GatewayTargetDeviceFailedToRespond)
    );
}
