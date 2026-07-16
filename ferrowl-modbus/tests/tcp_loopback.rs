//! End-to-end TCP test: a ferrowl Modbus TCP server and client talk over a
//! loopback socket. Drives the shared client loop (`client_core`) through every
//! read function code and every write command, plus graceful termination.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Command, FunctionCode, Key, Operation, SlaveKey};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use parking_lot::Mutex;
use parking_lot::RwLock as MemLock;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn key(kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey { slave_id: 1, kind })
}

/// A no-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_s: String| async move {}
}

/// A log sink that records every line, so a test can assert on what the client logged.
/// `LogFn + Clone` is satisfied by a move-closure capturing an `Arc`.
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

/// An OS-assigned free TCP port (bind to :0, read the port, drop the listener).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn config(port: u16) -> tcp::Config {
    tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
    }
}

fn config_no_reconnect(port: u16) -> tcp::Config {
    tcp::Config {
        reconnect: false,
        ..config(port)
    }
}

/// Server memory seeded with distinct values in all four register tables.
fn server_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::Coil),
        &MemKind::ReadWrite(CellType::Coil),
        &[Range::new(0, 8)],
    );
    mem.write(
        key(RegKind::Coil),
        &CellType::Coil,
        &Range::new(0, 4),
        &[1, 0, 1, 0],
    )
    .unwrap();
    mem.add_ranges(
        key(RegKind::DiscreteInput),
        &MemKind::ReadWrite(CellType::Coil),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(RegKind::DiscreteInput),
        &CellType::Coil,
        &Range::new(0, 4),
        &[0, 1, 1, 0],
    )
    .unwrap();
    mem.add_ranges(
        key(RegKind::InputRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.write(
        key(RegKind::InputRegister),
        &CellType::Register,
        &Range::new(0, 4),
        &[100, 200, 300, 400],
    )
    .unwrap();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 8)],
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

/// Client memory with the same regions declared but no values (the client fills them from reads).
fn client_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::Coil),
        &MemKind::ReadWrite(CellType::Coil),
        &[Range::new(0, 8)],
    );
    mem.add_ranges(
        key(RegKind::DiscreteInput),
        &MemKind::ReadWrite(CellType::Coil),
        &[Range::new(0, 4)],
    );
    mem.add_ranges(
        key(RegKind::InputRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 8)],
    );
    Arc::new(MemLock::new(mem))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-035 — the client polls every read operation and writes each result into the shared store
/// (and accepts write commands, MB-R-046, and terminates gracefully, MB-R-049).
/// MB-R-037 — polling advances round-robin, so all four operations are read in one pass.
/// MB-R-039 — `interval_ms` of 0 (this config) is treated as a fast tick rather than rejected.
/// MB-R-041 — the poll loop issues exactly the four read function codes (coils, discrete inputs, input registers, holding registers).
async fn tcp_client_polls_server_and_executes_commands() {
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    // Start the server.
    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem.clone())
        .spawn(sink())
        .await
        .expect("server failed to start");

    // Operations cover every read function code the client supports.
    let operations = Arc::new(RwLock::new(vec![
        Operation {
            slave_id: 1,
            fn_code: FunctionCode::ReadCoils,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: 1,
            fn_code: FunctionCode::ReadDiscreteInputs,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: 1,
            fn_code: FunctionCode::ReadInputRegisters,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: 1,
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 4),
        },
    ]));

    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    // Let the client poll every operation at least once.
    sleep(Duration::from_millis(800)).await;

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
        assert_eq!(
            g.read(
                key(RegKind::InputRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
            vec![100, 200, 300, 400]
        );
        assert_eq!(
            g.read(key(RegKind::Coil), &CellType::Coil, &Range::new(0, 4))
                .unwrap(),
            vec![1, 0, 1, 0]
        );
        assert_eq!(
            g.read(
                key(RegKind::DiscreteInput),
                &CellType::Coil,
                &Range::new(0, 4)
            )
            .unwrap(),
            vec![0, 1, 1, 0]
        );
    }

    // Exercise every write command against the server.
    tx.send(Command::WriteSingleRegister(1, 0, 99))
        .await
        .unwrap();
    tx.send(Command::WriteMultipleRegister(1, 1, vec![5, 6]))
        .await
        .unwrap();
    tx.send(Command::WriteSingleCoil(1, 5, true)).await.unwrap();
    tx.send(Command::WriteMultipleCoils(1, 6, vec![true, false]))
        .await
        .unwrap();
    sleep(Duration::from_millis(600)).await;

    {
        let g = srv_mem.read();
        assert_eq!(
            g.read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 3)
            )
            .unwrap(),
            vec![99, 5, 6]
        );
        assert_eq!(
            g.read(key(RegKind::Coil), &CellType::Coil, &Range::new(5, 3))
                .unwrap(),
            vec![1, 1, 0]
        );
    }

    // Graceful termination returns Ok and ends the client task.
    tx.send(Command::Terminate).await.unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("client did not terminate in time")
        .expect("client task panicked");
    assert!(joined.is_ok());

    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-043 — Modbus exceptions do not disconnect the client; it retries then skips the operation
/// (and rejected write commands are logged without disconnecting, MB-R-047).
async fn tcp_client_handles_server_rejections() {
    let port = free_port();
    // Server with no registered regions: every request for slave 1 is rejected.
    let srv_mem: Mem = Arc::new(MemLock::new(Memory::<Key<SlaveKey>>::default()));
    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 2),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        client_mem(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    // Several poll cycles let the exception-retry counter pass MAX_RETRIES.
    sleep(Duration::from_millis(800)).await;

    // Writes the server rejects -> the "invalid" command branches.
    tx.send(Command::WriteSingleRegister(1, 0, 1))
        .await
        .unwrap();
    tx.send(Command::WriteMultipleRegister(1, 0, vec![1, 2]))
        .await
        .unwrap();
    tx.send(Command::WriteSingleCoil(1, 0, true)).await.unwrap();
    tx.send(Command::WriteMultipleCoils(1, 0, vec![true]))
        .await
        .unwrap();
    sleep(Duration::from_millis(600)).await;

    tx.send(Command::Terminate).await.unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("client did not terminate in time")
        .expect("client task panicked");
    assert!(joined.is_ok());
    server.abort();
}

#[tokio::test]
/// MB-R-069 — an `ip`/`port` pair that does not parse as a socket address fails with a TCP address
/// error, for both the client and the server.
async fn tcp_unparseable_address_is_error() {
    use ferrowl_modbus::{Error, TcpError};

    let mut bad = config(502);
    bad.ip = "not.an.ip.address".to_string();

    // Client side (`Client` isn't `Debug`, so match the result rather than `unwrap_err`).
    assert!(matches!(
        tcp::Client::connect(&bad).await,
        Err(Error::Tcp(TcpError::Address(_)))
    ));

    // Server side.
    let mem: Mem = Arc::new(MemLock::new(Memory::<Key<SlaveKey>>::default()));
    let server_err = tcp::ServerBuilder::new(Arc::new(RwLock::new(bad)), mem)
        .spawn(sink())
        .await
        .unwrap_err();
    assert!(matches!(server_err, Error::Tcp(TcpError::Address(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-070 — a TCP server accepts connections in a loop, serving multiple concurrent clients
/// against the same shared store.
async fn tcp_server_serves_concurrent_clients() {
    let port = free_port();
    let srv_mem = server_mem();

    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    // Two independent clients connect at the same time and both read from the one server.
    let ops = || {
        Arc::new(RwLock::new(vec![Operation {
            slave_id: 1,
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 4),
        }]))
    };
    let mem_a = client_mem();
    let mem_b = client_mem();
    let (tx_a, rx_a) = mpsc::channel::<Command>(16);
    let (tx_b, rx_b) = mpsc::channel::<Command>(16);
    let client_a =
        tcp::ClientBuilder::new(Arc::new(RwLock::new(config(port))), ops(), mem_a.clone())
            .spawn(rx_a, sink(), sink())
            .await
            .expect("client A failed to connect");
    let client_b =
        tcp::ClientBuilder::new(Arc::new(RwLock::new(config(port))), ops(), mem_b.clone())
            .spawn(rx_b, sink(), sink())
            .await
            .expect("client B failed to connect");

    sleep(Duration::from_millis(600)).await;

    for m in [&mem_a, &mem_b] {
        assert_eq!(
            m.read()
                .read(
                    key(RegKind::HoldingRegister),
                    &CellType::Register,
                    &Range::new(0, 4)
                )
                .unwrap(),
            vec![10, 20, 30, 40]
        );
    }

    tx_a.send(Command::Terminate).await.unwrap();
    tx_b.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client_a).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), client_b).await;
    server.abort();
}

#[tokio::test]
/// MB-R-068 — a TCP client connect attempt to a port with no listener fails.
async fn tcp_client_connect_refused_is_error() {
    // Nothing is listening on this port, so the connect fails.
    let port = free_port();
    assert!(tcp::Client::connect(&config(port)).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// MB-R-071 — failure to bind the TCP listen address fails the server's start and surfaces the error.
async fn tcp_server_bind_conflict_is_error() {
    let port = free_port();
    // Occupy the port so the server's bind fails.
    let _occupier = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    let mem: Mem = Arc::new(MemLock::new(Memory::<Key<SlaveKey>>::default()));
    let res = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), mem)
        .spawn(sink())
        .await;
    assert!(res.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-055 — with reconnect disabled, a failed connect ends the client task with that error.
async fn tcp_client_reconnect_false_dies_on_refused_connect() {
    // No listener; with reconnect off the spawned task's join result carries the connect error
    // instead of retrying forever.
    let port = free_port();
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 2),
    }]));
    let (_tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config_no_reconnect(port))),
        operations,
        client_mem(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn itself always succeeds now; the connect error surfaces from the task");

    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("client task did not finish in time")
        .expect("client task panicked");
    assert!(joined.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-050 — with reconnect enabled, a refused connect is retried after a backoff and connects once a listener appears.
async fn tcp_client_reconnect_true_connects_once_a_listener_appears() {
    // Nothing is listening yet: the client's first connect attempt fails. With reconnect on it
    // keeps retrying in the background; once a server starts on the port, it should connect and
    // start reading.
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    // Let the first (failing) connect attempt happen before the server exists.
    sleep(Duration::from_millis(200)).await;

    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    // The 1s initial backoff plus poll delay must elapse before the client retries and reads.
    sleep(Duration::from_millis(2000)).await;

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
    let joined = tokio::time::timeout(Duration::from_secs(5), client)
        .await
        .expect("client did not terminate in time")
        .expect("client task panicked");
    assert!(joined.is_ok());
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-053 — Terminate aborts a reconnect backoff wait immediately and ends the client task with success.
async fn tcp_client_terminate_during_backoff_exits_promptly() {
    // No listener, so the client sits in its reconnect backoff (up to 1s initially). Sending
    // Terminate must abort that wait immediately rather than sleeping it out.
    let port = free_port();
    let operations = Arc::new(RwLock::new(vec![]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        client_mem(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("spawn itself always succeeds now");

    // Give the first failed connect attempt time to happen and enter the backoff wait.
    sleep(Duration::from_millis(100)).await;
    tx.send(Command::Terminate).await.unwrap();

    // Well under the 1s initial backoff: proves Terminate interrupts the wait rather than
    // sleeping it out.
    let joined = tokio::time::timeout(Duration::from_millis(500), client)
        .await
        .expect("Terminate did not interrupt the reconnect backoff promptly")
        .expect("client task panicked");
    assert!(joined.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-036 — the operation list is shared and mutable at runtime; an operation added after the
/// client is polling is picked up on a later poll cycle without any reconnect.
async fn tcp_client_operation_list_mutated_at_runtime() {
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    // Start with a single operation reading the holding registers.
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations.clone(),
        cli_mem.clone(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    sleep(Duration::from_millis(300)).await;
    // The input-register table has not been read yet: still zeros in the client store.
    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::InputRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![0, 0, 0, 0]
    );

    // Add an input-register operation at runtime — no reconnect.
    operations.write().await.push(Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadInputRegisters,
        range: Range::new(0, 4),
    });
    sleep(Duration::from_millis(400)).await;

    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::InputRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![100, 200, 300, 400]
    );

    tx.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-056 — the connection settings (here the endpoint) are re-read from the shared config on
/// every connection attempt, so an edit takes effect on the next reconnect.
async fn tcp_client_rereads_config_on_reconnect() {
    let good_port = free_port();
    let bad_port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    // A server listens on `good_port`; the client is initially pointed at `bad_port` (no listener).
    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(good_port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    let shared_cfg = Arc::new(RwLock::new(config(bad_port)));
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(shared_cfg.clone(), operations, cli_mem.clone())
        .spawn(rx, sink(), sink())
        .await
        .expect("spawn succeeds; first connect fails against the empty port");

    // First connect attempt fails; while it backs off, repoint the config at the live server.
    sleep(Duration::from_millis(200)).await;
    shared_cfg.write().await.port = good_port;

    // The 1 s initial backoff plus a poll tick must elapse before the re-read connect and read.
    sleep(Duration::from_millis(2000)).await;
    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![10, 20, 30, 40]
    );

    tx.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-052 — the backoff resets to 1 s after a connection run that got at least one read through,
/// so the reconnect logged after a successful run is "1s" even though the backoff had already grown.
async fn tcp_client_backoff_resets_after_successful_run() {
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let (log, lines) = capturing();
    // No server yet: the first connect fails and the backoff grows to 2 s.
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, log, sink())
    .await
    .expect("spawn succeeds; first connect fails");

    // Bring the server up during the first (1 s) backoff so the second attempt connects and reads.
    sleep(Duration::from_millis(500)).await;
    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    // Let the client connect and get at least one read through (marks the run successful).
    sleep(Duration::from_millis(2000)).await;
    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![10, 20, 30, 40]
    );

    // Drop the server: the run ends with a transport error, and because it read successfully the
    // backoff is reset to 1 s.
    server.abort();
    sleep(Duration::from_millis(1500)).await;

    let logged = lines.lock().clone();
    let first_success = logged
        .iter()
        .position(|l| l.contains("successful"))
        .expect("expected a successful read");
    let reconnect_after_success = logged[first_success..]
        .iter()
        .find(|l| l.contains("Reconnecting in"))
        .expect("expected a reconnect log after the successful run");
    // The backoff had already grown to 2 s from the first failed connect; the reset brings the
    // post-success reconnect back to 1 s.
    assert!(
        reconnect_after_success.contains("in 1s"),
        "post-success reconnect backoff was not reset to 1s: {reconnect_after_success}"
    );

    tx.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-048 — each read addresses the slave id carried by the operation, independent of any slave
/// id configured on the transport (the TCP transport configures none).
async fn tcp_client_addresses_operation_slave_id() {
    let port = free_port();

    // Server memory declared only under slave id 7. A request for any other slave finds no region.
    let k7 = || {
        Key::new(SlaveKey {
            slave_id: 7,
            kind: RegKind::HoldingRegister,
        })
    };
    let mut sm = Memory::<Key<SlaveKey>>::default();
    sm.add_ranges(
        k7(),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    sm.write(
        k7(),
        &CellType::Register,
        &Range::new(0, 4),
        &[11, 22, 33, 44],
    )
    .unwrap();
    let srv_mem: Mem = Arc::new(MemLock::new(sm));

    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    // Client store keyed on slave 7 too; the operation targets slave 7.
    let mut cm = Memory::<Key<SlaveKey>>::default();
    cm.add_ranges(
        k7(),
        &MemKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    let cli_mem: Mem = Arc::new(MemLock::new(cm));

    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 7,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, sink(), sink())
    .await
    .expect("client failed to connect");

    sleep(Duration::from_millis(400)).await;
    // The read only succeeds if the request was addressed to slave 7 (set from the operation).
    assert_eq!(
        cli_mem
            .read()
            .read(k7(), &CellType::Register, &Range::new(0, 4))
            .unwrap(),
        vec![11, 22, 33, 44]
    );

    tx.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-038 — the client waits `delay_ms` before its first poll on a connection.
async fn tcp_client_delays_before_first_poll() {
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem)
        .spawn(sink())
        .await
        .expect("server failed to start");

    // A long start delay: nothing should be read until it elapses.
    let cfg = tcp::Config {
        delay_ms: 600,
        ..config(port)
    };
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(Arc::new(RwLock::new(cfg)), operations, cli_mem.clone())
        .spawn(rx, sink(), sink())
        .await
        .expect("client failed to connect");

    // Well before the 600ms delay elapses: nothing polled yet.
    sleep(Duration::from_millis(250)).await;
    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![0, 0, 0, 0]
    );

    // After the delay plus a poll tick: the values are in.
    sleep(Duration::from_millis(600)).await;
    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![10, 20, 30, 40]
    );

    tx.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-040 — every read is bounded by `timeout_ms`; a server that accepts the connection but never
/// answers makes the read time out, which (with reconnect off) ends the client task with an error.
async fn tcp_client_read_times_out_when_server_silent() {
    // A raw TCP listener that accepts connections but never speaks Modbus.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let silent = tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                held.push(stream); // keep it open; never reply
            }
        }
    });

    let cfg = tcp::Config {
        timeout_ms: 300,
        ..config_no_reconnect(port)
    };
    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 2),
    }]));
    let (_tx, rx) = mpsc::channel::<Command>(16);
    let client = tcp::ClientBuilder::new(Arc::new(RwLock::new(cfg)), operations, client_mem())
        .spawn(rx, sink(), sink())
        .await
        .expect("connect (TCP handshake) succeeds against the silent listener");

    // timeout_ms is 300ms; the task must end with an error well before this bound.
    let joined = tokio::time::timeout(Duration::from_secs(3), client)
        .await
        .expect("read did not time out within the bound")
        .expect("client task panicked");
    assert!(joined.is_err());
    silent.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-044 — a successful read resets the retry counter, so an operation that was failing and then
/// recovers keeps polling successfully instead of staying skipped.
async fn tcp_client_success_resets_retry_counter() {
    let port = free_port();
    // Server starts with no region declared: every read for slave 1 is rejected (exceptions).
    let cli_mem = client_mem();
    let srv_mem: Mem = Arc::new(MemLock::new(Memory::<Key<SlaveKey>>::default()));
    let server = tcp::ServerBuilder::new(Arc::new(RwLock::new(config(port))), srv_mem.clone())
        .spawn(sink())
        .await
        .expect("server failed to start");

    let operations = Arc::new(RwLock::new(vec![Operation {
        slave_id: 1,
        fn_code: FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 4),
    }]));
    let (tx, rx) = mpsc::channel::<Command>(16);
    let (log, lines) = capturing();
    let client = tcp::ClientBuilder::new(
        Arc::new(RwLock::new(config(port))),
        operations,
        cli_mem.clone(),
    )
    .spawn(rx, log, sink())
    .await
    .expect("client failed to connect");

    // Let several exception-retry cycles run against the region-less server.
    sleep(Duration::from_millis(500)).await;

    // Now declare and seed the region: reads start succeeding, which resets the retry counter.
    {
        let mut g = srv_mem.write();
        g.add_ranges(
            key(RegKind::HoldingRegister),
            &MemKind::ReadWrite(CellType::Register),
            &[Range::new(0, 4)],
        );
        g.write(
            key(RegKind::HoldingRegister),
            &CellType::Register,
            &Range::new(0, 4),
            &[10, 20, 30, 40],
        )
        .unwrap();
    }
    sleep(Duration::from_millis(500)).await;

    // The recovered operation keeps polling and fills the client store.
    assert_eq!(
        cli_mem
            .read()
            .read(
                key(RegKind::HoldingRegister),
                &CellType::Register,
                &Range::new(0, 4)
            )
            .unwrap(),
        vec![10, 20, 30, 40]
    );
    // A "successful" read line was logged after recovery, proving the counter was reset and the
    // operation resumed rather than staying permanently invalid.
    assert!(
        lines.lock().iter().any(|l| l.contains("successful")),
        "expected a successful read after recovery"
    );

    tx.send(Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), client).await;
    server.abort();
}
