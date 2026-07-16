//! RTU (serial) transport tests. A real serial loopback needs hardware (or a named
//! PTY the RTU builders can open by path), which isn't portable in CI, so these cover
//! the serial-open failure paths — which is where the RTU-specific lifecycle behavior
//! (open-once-at-start for the server, reconnect-on-open-failure for the client) is
//! observable.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use ferrowl_modbus::rtu;
use ferrowl_modbus::{Command, Error, FunctionCode, Key, Operation, SerialError, SlaveKey};
use ferrowl_store::{Memory, Range};
use parking_lot::RwLock as MemLock;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

fn empty_mem() -> Mem {
    Arc::new(MemLock::new(Memory::<Key<SlaveKey>>::default()))
}

/// A serial path that cannot be opened, so `SerialStream::open` fails.
fn bad_config(reconnect: bool) -> rtu::Config {
    rtu::Config {
        path: "/nonexistent/ferrowl-no-such-serial-port".to_string(),
        baud_rate: 115200,
        slave: 1,
        parity: None,
        data_bits: None,
        stop_bits: None,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-075 — failure to open the serial port fails an RTU server's start with a serial error.
/// MB-R-074 — the RTU server opens the port once at start (there is no accept loop deferring it), so
/// an unopenable port surfaces at `spawn` rather than being retried per connection.
async fn rtu_server_open_failure_fails_start() {
    let res = rtu::ServerBuilder::new(Arc::new(RwLock::new(bad_config(false))), empty_mem())
        .spawn(sink())
        .await;
    assert!(matches!(res, Err(Error::Serial(SerialError::Error(_)))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-075 — for an RTU client, a serial-open failure is a failed connection attempt; with
/// reconnect disabled it ends the client task with the error.
async fn rtu_client_open_failure_reconnect_false_dies() {
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 2),
    }]));
    let (_tx, rx) = mpsc::channel::<Command>(16);
    let client = rtu::ClientBuilder::new(
        Arc::new(RwLock::new(bad_config(false))),
        operations,
        empty_mem(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn succeeds; the open error surfaces from the task");

    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("client task did not finish in time")
        .expect("client task panicked");
    assert!(joined.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-075 — for an RTU client, a serial-open failure with reconnect enabled is subject to the
/// reconnect rules: the task keeps retrying rather than dying, and Terminate ends it cleanly.
async fn rtu_client_open_failure_reconnect_true_retries() {
    let operations = Arc::new(RwLock::new(vec![]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = rtu::ClientBuilder::new(
        Arc::new(RwLock::new(bad_config(true))),
        operations,
        empty_mem(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn succeeds");

    // The open keeps failing; with reconnect on, the task must still be alive (backing off), not
    // finished. Terminate then ends it with success.
    sleep(Duration::from_millis(200)).await;
    tx.send(Command::Terminate).await.unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("Terminate did not end the retrying client in time")
        .expect("client task panicked");
    assert!(joined.is_ok());
}
