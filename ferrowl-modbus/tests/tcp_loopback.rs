//! End-to-end TCP test: a ferrowl Modbus TCP server and client talk over a
//! loopback socket. Drives the shared client loop (`client_core`) through every
//! read function code and every write command, plus graceful termination.

use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Command, FunctionCode, Key, Operation, SlaveKey};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
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
async fn tcp_client_connect_refused_is_error() {
    // Nothing is listening on this port, so the connect fails.
    let port = free_port();
    assert!(tcp::Client::connect(&config(port)).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
