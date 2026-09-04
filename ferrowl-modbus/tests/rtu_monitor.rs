//! RTU monitor open-failure tests, mirroring `rtu_serial.rs`'s scope note: a real serial
//! loopback needs hardware, which isn't portable in CI, so these cover the serial-open
//! failure path — where the retry/backoff lifecycle (MB-R-141, MB-R-130–134) is observable.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use ferrowl_modbus::ServerCommand;
use ferrowl_modbus::monitor::{ObservedTable, RecordLog};
use ferrowl_modbus::rtu;
use parking_lot::RwLock as MemLock;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

fn empty_table() -> ferrowl_modbus::monitor::SharedObservedTable {
    Arc::new(MemLock::new(ObservedTable::default()))
}

fn empty_records() -> ferrowl_modbus::monitor::SharedRecordLog {
    Arc::new(MemLock::new(RecordLog::default()))
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
/// MB-R-192 — with `reconnect` enabled (the default), a serial-open failure does not fail an
/// RTU monitor's start: `spawn()` returns `Ok(handle)`, and the task keeps retrying the open on
/// the shared backoff policy instead of ending.
async fn it_monitor_open_failure_retries_while_reconnect_enabled() {
    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _open) = rtu::MonitorBuilder::new(
        Arc::new(RwLock::new(bad_config(true))),
        empty_table(),
        empty_records(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn always returns Ok");
    sleep(Duration::from_millis(200)).await;
    assert!(
        !handle.is_finished(),
        "an open failure with reconnect enabled must keep retrying, not end the task"
    );
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-192 — with `reconnect` disabled, a serial-open failure ends the monitor: `spawn()`
/// still returns `Ok(handle)`, but the joined task carries the serial error.
async fn it_monitor_open_failure_reconnect_false_ends_task() {
    let (_tx, rx) = mpsc::channel::<ServerCommand>(1);
    let (handle, _open) = rtu::MonitorBuilder::new(
        Arc::new(RwLock::new(bad_config(false))),
        empty_table(),
        empty_records(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn always returns Ok");
    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("task should end promptly, not retry, with reconnect disabled")
        .expect("task must not panic");
    assert!(matches!(
        result,
        Err(ferrowl_modbus::Error::Serial(
            ferrowl_modbus::SerialError::Error(_)
        ))
    ));
}
