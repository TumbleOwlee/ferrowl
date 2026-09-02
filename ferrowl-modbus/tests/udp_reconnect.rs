//! Udp server bind-failure retry tests (MB-R-120 revised, MB-R-130–134). Mirrors
//! `tcp_reconnect.rs`: a bind failure now retries with backoff under `reconnect: true` instead
//! of failing `spawn()` synchronously.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ferrowl_modbus::udp;
use ferrowl_modbus::{Error, ServerCommand, SlaveKey};
use ferrowl_store::Memory;
use ferrowl_test_support::reserve_udp_port;
use parking_lot::RwLock as MemLock;
use rust_modbus::{
    Address as RmAddress, Client as RmClient, Quantity as RmQuantity, UdpConfig, connect_udp,
};
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<ferrowl_modbus::Key<SlaveKey>>>>;

fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

fn empty_mem() -> Mem {
    Arc::new(MemLock::new(Memory::default()))
}

fn config(port: u16, reconnect: bool) -> udp::Config {
    udp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-120, MB-R-130 — with `reconnect` enabled (the default), a Udp bind failure
/// does not fail the server's start: `spawn()` returns `Ok(handle)`, the task keeps retrying
/// the bind on the shared backoff policy, and once the port frees up a real request round-trips.
async fn udp_server_bind_failure_retries_then_succeeds() {
    let occupier = reserve_udp_port();
    let port = occupier.port();

    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) =
        udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(config(port, true))), empty_mem())
            .spawn(rx, sink(), sink())
            .await
            .expect("spawn always returns Ok");

    sleep(Duration::from_millis(200)).await;
    assert!(
        !handle.is_finished(),
        "a bind failure with reconnect enabled must keep retrying, not end the task"
    );

    // Free the port and give the backoff-driven retry (default initial 1s) time to succeed.
    drop(occupier);
    sleep(Duration::from_millis(1500)).await;

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let transport = connect_udp(addr, UdpConfig::default())
        .await
        .expect("associates once the server has bound");
    let mut client: rust_modbus::UdpClient = RmClient::new(transport);
    // No registers are declared in `empty_mem()`, so an illegal-data-address exception is the
    // expected (and sufficient) proof the server answered at all.
    let result = client
        .read_holding_registers(rust_modbus::UnitId(1), RmAddress(0), RmQuantity(1))
        .await;
    assert!(
        result.is_err(),
        "server should answer (with an exception), proving it bound and is serving"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-120, MB-R-134 — with `reconnect` disabled, a Udp bind failure fails the
/// server: `spawn()` still returns `Ok(handle)`, but the joined task carries the bind error.
async fn udp_server_bind_failure_reconnect_false_ends_task() {
    let occupier = reserve_udp_port();
    let port = occupier.port();

    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = udp::ServerBuilder::<SlaveKey>::new(
        Arc::new(RwLock::new(config(port, false))),
        empty_mem(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn always returns Ok");

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("task should end promptly, not retry, with reconnect disabled")
        .expect("task must not panic");
    assert!(matches!(result, Err(Error::Server(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-133 — a `ServerCommand::Terminate` sent while the Udp server is backing off from a bind
/// failure ends the task gracefully (`Ok(())`), not with the bind error.
async fn udp_server_terminate_while_backing_off_ends_task_ok() {
    let occupier = reserve_udp_port();
    let port = occupier.port();

    let (tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) =
        udp::ServerBuilder::<SlaveKey>::new(Arc::new(RwLock::new(config(port, true))), empty_mem())
            .spawn(rx, sink(), sink())
            .await
            .expect("spawn always returns Ok");

    sleep(Duration::from_millis(100)).await;
    tx.send(ServerCommand::Terminate).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("Terminate did not end the retrying server in time")
        .expect("task must not panic");
    assert!(result.is_ok());
}
