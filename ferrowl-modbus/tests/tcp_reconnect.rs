//! Server-side bind-failure retry for TCP-framed server transports (MB-R-071 revised,
//! MB-R-114, MB-R-126, MB-R-130-134). Kept apart from `tcp_tls_server.rs`'s TLS-configuration
//! tests: this file is about the retry/backoff axis, not certificate handling.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::{Address, Key, ServerCommand, SlaveKey, UnitId};
use ferrowl_store::{CellKind, CellType, Memory, Range};
use ferrowl_test_support::reserve_tcp_port;
use parking_lot::RwLock as MemLock;
use tokio::sync::{RwLock, mpsc};

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

fn key(kind: RegKind) -> Key<SlaveKey> {
    Key::new(SlaveKey {
        slave_id: UnitId(1),
        kind,
    })
}

/// A no-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_modbus::LogFn + Clone {
    |_level: ferrowl_modbus::Level, _s: String| async move {}
}

fn server_mem() -> Mem {
    let mut mem = Memory::<Key<SlaveKey>>::default();
    mem.add_ranges(
        key(RegKind::HoldingRegister),
        &CellKind::read_write(CellType::Register),
        &[Range::new(0, 4)],
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

fn tcp_config(port: u16, reconnect: bool) -> ferrowl_modbus::tcp::Config {
    ferrowl_modbus::tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect,
        tls: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-071, MB-R-130, NF-R-059 — with `reconnect` enabled (the default), a TCP server whose listen port is
/// already occupied does not fail its start: `spawn()` still returns `Ok`, the task keeps
/// retrying the bind, and once the occupier drops, the very next attempt succeeds and a real
/// client can connect.
async fn tcp_server_bind_failure_retries_then_succeeds() {
    let occupier = reserve_tcp_port();
    let port = occupier.port();

    let (_sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, sink(), sink())
    .await
    .expect("spawn always returns Ok; the bind failure surfaces from the task instead");

    // Bind is failing and retrying, not ending the task.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !handle.is_finished(),
        "task must still be retrying the bind"
    );

    drop(occupier);
    // The default backoff's first wait is 1s (MB-R-051); give it enough room to retry and bind.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let mut client =
        rust_modbus::Client::<_, rust_modbus::Tcp>::new(rust_modbus::FrameTransport::new(
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("server should have rebound after the occupier dropped"),
        ));
    let registers = client
        .read_holding_registers(UnitId(1), Address(0), rust_modbus::Quantity(2))
        .await
        .expect("a real client should be able to read once the retry rebinds");
    assert_eq!(
        registers,
        vec![
            rust_modbus::RegisterValue(10),
            rust_modbus::RegisterValue(20)
        ]
    );

    handle.abort();
}

#[tokio::test]
/// MB-R-134, MB-R-257 — with `reconnect` disabled, a TCP server bind failure (the occupier holds the port, so the bind is exclusive) ends the task with the
/// error: `spawn()` itself still returns `Ok(handle)`, but awaiting `handle` resolves to
/// `Err(Error::Server(_))`.
async fn tcp_server_bind_failure_reconnect_false_ends_task() {
    let occupier = reserve_tcp_port();
    let port = occupier.port();

    let (_sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, false))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, sink(), sink())
    .await
    .expect("spawn always returns Ok");

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("task should end promptly, not retry, with reconnect disabled")
        .expect("task must not panic");
    let Err(ferrowl_modbus::Error::Server(e)) = result else {
        panic!("expected a server error, got {result:?}");
    };
    assert!(
        format!("{e:?}").contains("AddrInUse"),
        "not address-in-use: {e:?}"
    );

    drop(occupier);
}

#[tokio::test]
/// MB-R-133 — `ServerCommand::Terminate`, sent while the server is backing off after a bind
/// failure, ends the task promptly with `Ok(())` rather than waiting out the whole backoff.
async fn tcp_server_terminate_while_backing_off_ends_task_ok() {
    let occupier = reserve_tcp_port();
    let port = occupier.port();

    let (sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, sink(), sink())
    .await
    .expect("spawn always returns Ok");

    // Give the first bind attempt (and its failure) time to happen, so the task is certainly
    // in its backoff wait by the time Terminate is sent.
    tokio::time::sleep(Duration::from_millis(100)).await;
    sender.send(ServerCommand::Terminate).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("Terminate must abort the backoff wait promptly, not wait it out")
        .expect("task must not panic");
    assert!(result.is_ok());

    drop(occupier);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-114 — an RtuOverTcp server (reuses `tcp::Config`, MB-R-113) retries a bind-failure the
/// same way TCP does.
async fn rtu_over_tcp_server_bind_failure_retries_then_succeeds() {
    let occupier = reserve_tcp_port();
    let port = occupier.port();

    let (_sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = ferrowl_modbus::rtu_over_tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, sink(), sink())
    .await
    .expect("spawn always returns Ok");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !handle.is_finished(),
        "task must still be retrying the bind"
    );

    drop(occupier);
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let mut client =
        rust_modbus::Client::<_, rust_modbus::RtuOverTcp>::new(rust_modbus::FrameTransport::new(
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("server should have rebound after the occupier dropped"),
        ));
    let registers = client
        .read_holding_registers(UnitId(1), Address(0), rust_modbus::Quantity(2))
        .await
        .expect("a real client should be able to read once the retry rebinds");
    assert_eq!(
        registers,
        vec![
            rust_modbus::RegisterValue(10),
            rust_modbus::RegisterValue(20)
        ]
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-126 — an AsciiOverTcp server (reuses `tcp::Config`, MB-R-113) retries a bind-failure the
/// same way TCP does.
async fn ascii_over_tcp_server_bind_failure_retries_then_succeeds() {
    let occupier = reserve_tcp_port();
    let port = occupier.port();

    let (_sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = ferrowl_modbus::ascii_over_tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, sink(), sink())
    .await
    .expect("spawn always returns Ok");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !handle.is_finished(),
        "task must still be retrying the bind"
    );

    drop(occupier);
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let mut client =
        rust_modbus::Client::<_, rust_modbus::Ascii>::new(rust_modbus::FrameTransport::new(
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("server should have rebound after the occupier dropped"),
        ));
    let registers = client
        .read_holding_registers(UnitId(1), Address(0), rust_modbus::Quantity(2))
        .await
        .expect("a real client should be able to read once the retry rebinds");
    assert_eq!(
        registers,
        vec![
            rust_modbus::RegisterValue(10),
            rust_modbus::RegisterValue(20)
        ]
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-221, MB-E-090 — a terminate arriving around a server's listener bind ends the task with
/// success rather than waiting for the bind to complete.
async fn it_terminate_while_server_bind_pending_ends_task_ok() {
    let port = reserve_tcp_port().release();

    let (sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound_addr) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, sink(), sink())
    .await
    .expect("spawn always returns Ok");

    sender.send(ServerCommand::Terminate).await.unwrap();

    let result = tokio::time::timeout(Duration::from_millis(500), handle)
        .await
        .expect("terminate around the bind must not hang")
        .expect("task must not panic");
    assert!(result.is_ok(), "the server task must end with success");
}

type Lines = Arc<parking_lot::Mutex<Vec<(ferrowl_modbus::Level, String)>>>;

/// A log sink recording each line with its level.
fn capturing_levels() -> (impl ferrowl_modbus::LogFn + Clone, Lines) {
    let log = Lines::default();
    let sink = log.clone();
    let f = move |level: ferrowl_modbus::Level, s: String| {
        let sink = sink.clone();
        async move {
            sink.lock().push((level, s));
        }
    };
    (f, log)
}

fn has(lines: &Lines, level: ferrowl_modbus::Level, needle: &str) -> bool {
    lines
        .lock()
        .iter()
        .any(|(l, s)| *l == level && s.contains(needle))
}

fn read_op() -> Arc<RwLock<Vec<ferrowl_modbus::Operation>>> {
    Arc::new(RwLock::new(vec![ferrowl_modbus::Operation {
        slave_id: UnitId(1),
        fn_code: ferrowl_modbus::FunctionCode::ReadHoldingRegisters,
        range: Range::new(0, 2),
    }]))
}

/// Spawn a client against `port` with the given log and status sinks.
async fn spawn_client(
    port: u16,
    reconnect: bool,
    log: impl ferrowl_modbus::LogFn + Clone,
    status: impl ferrowl_modbus::LogFn + Clone,
) -> (
    tokio::task::JoinHandle<Result<(), ferrowl_modbus::Error>>,
    mpsc::Sender<ferrowl_modbus::Command>,
) {
    let (tx, rx) = mpsc::channel(16);
    let (handle, _connected) = ferrowl_modbus::tcp::ClientBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, reconnect))),
        read_op(),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(rx, log, status)
    .await
    .expect("spawn always returns Ok");
    (handle, tx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-258, MB-R-260, MB-R-262, MB-E-098 — a refused connect attempt logs at Error whatever
/// words the OS error text holds, and the following backoff announcement logs at Info.
async fn it_client_connect_refused_logs_error() {
    use ferrowl_modbus::Level;
    // Bound but never `listen()`ed: connects are refused while the binding stays reserved.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = socket.local_addr().unwrap().port();
    let (log, lines) = capturing_levels();
    let (handle, tx) = spawn_client(port, true, log, sink()).await;

    ferrowl_test_support::wait_until(
        "refused connect and backoff announcement",
        Duration::from_millis(50),
        Duration::from_secs(10),
        || {
            (has(&lines, Level::Error, "refused") && has(&lines, Level::Info, "Reconnecting in"))
                .then_some(())
        },
    )
    .await;
    assert!(
        !lines
            .lock()
            .iter()
            .any(|(l, s)| *l != Level::Error && s.contains("refused")),
        "the refused-connect line must not be classified by its text: {:?}",
        lines.lock()
    );

    tx.send(ferrowl_modbus::Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    drop(socket);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-258, MB-R-263 — with `reconnect` disabled the line announcing that the task ends on a
/// failed connect carries Error.
async fn it_client_reconnect_disabled_logs_error() {
    use ferrowl_modbus::Level;
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = socket.local_addr().unwrap().port();
    let (log, lines) = capturing_levels();
    let (handle, _tx) = spawn_client(port, false, log, sink()).await;

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the task must end")
        .expect("task must not panic");
    assert!(result.is_err());
    assert!(has(&lines, Level::Error, "Reconnect disabled"));
    drop(socket);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-258, MB-R-261, MB-R-262, MB-R-264 — a peer that drops the connection mid-run logs the
/// failed request at Error, the lost connection at Warning, and the backoff at Info.
async fn it_client_lost_connection_levels() {
    use ferrowl_modbus::Level;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tokio::spawn(async move {
        loop {
            // Accept and drop at once: the client's first request meets a closed connection.
            let _ = listener.accept().await;
        }
    });
    let (log, lines) = capturing_levels();
    let (handle, tx) = spawn_client(port, true, log, sink()).await;

    ferrowl_test_support::wait_until(
        "lost connection logged",
        Duration::from_millis(50),
        Duration::from_secs(10),
        || {
            (has(&lines, Level::Error, "Disconnecting client")
                && has(&lines, Level::Warning, "Modbus error")
                && has(&lines, Level::Info, "Reconnecting in"))
            .then_some(())
        },
    )
    .await;

    tx.send(ferrowl_modbus::Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    acceptor.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-258, MB-R-266, MB-R-275 — terminating a connected client logs the graceful stop and the
/// disconnect status at Info.
async fn it_client_terminate_logs_info() {
    use ferrowl_modbus::Level;
    let port = reserve_tcp_port().release();
    let (_srv_tx, srv_rx) = mpsc::channel::<ServerCommand>(1);
    let (server, bound) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(srv_rx, sink(), sink())
    .await
    .unwrap();
    ferrowl_test_support::wait_until(
        "bind",
        Duration::from_millis(20),
        Duration::from_secs(10),
        || *bound.lock(),
    )
    .await;
    let (log, lines) = capturing_levels();
    let (status, status_lines) = capturing_levels();
    let (handle, tx) = spawn_client(port, true, log, status).await;
    ferrowl_test_support::wait_until(
        "first request",
        Duration::from_millis(20),
        Duration::from_secs(10),
        || has(&lines, Level::Info, "Perform").then_some(()),
    )
    .await;

    tx.send(ferrowl_modbus::Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(has(&lines, Level::Info, "Client gracefully terminated."));
    assert!(has(&status_lines, Level::Info, "Client disconnected"));
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-258, MB-R-279 — a command arriving while the client is disconnected and backing off is
/// dropped with a Warning line.
async fn it_client_command_dropped_while_backing_off_logs_warning() {
    use ferrowl_modbus::Level;
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = socket.local_addr().unwrap().port();
    let (log, lines) = capturing_levels();
    let (handle, tx) = spawn_client(port, true, log, sink()).await;

    ferrowl_test_support::wait_until(
        "backoff announced",
        Duration::from_millis(20),
        Duration::from_secs(10),
        || has(&lines, Level::Info, "Reconnecting in").then_some(()),
    )
    .await;
    tx.send(ferrowl_modbus::Command::WriteSingleRegister(
        UnitId(1),
        Address(0),
        ferrowl_modbus::Word(1),
    ))
    .await
    .unwrap();
    ferrowl_test_support::wait_until(
        "dropped command logged",
        Duration::from_millis(20),
        Duration::from_secs(10),
        || has(&lines, Level::Warning, "Command dropped").then_some(()),
    )
    .await;

    tx.send(ferrowl_modbus::Command::Terminate).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    drop(socket);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-256, MB-R-258, MB-R-267, UI-E-170 — while the port is held, every failed bind attempt
/// logs an Error naming the address; once the holder lets go the server logs that it is
/// listening, at Info.
async fn it_server_bind_failure_logs_error_then_listening_info() {
    use ferrowl_modbus::Level;
    let occupier = reserve_tcp_port();
    let port = occupier.port();
    let (log, lines) = capturing_levels();
    let (_sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, true))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, log, sink())
    .await
    .expect("spawn always returns Ok");

    let addr = format!("127.0.0.1:{port}");
    ferrowl_test_support::wait_until(
        "bind failure logged",
        Duration::from_millis(20),
        Duration::from_secs(10),
        || has(&lines, Level::Error, &addr).then_some(()),
    )
    .await;

    drop(occupier);
    ferrowl_test_support::wait_until(
        "listening logged after the port frees",
        Duration::from_millis(50),
        Duration::from_secs(10),
        || has(&lines, Level::Info, &format!("Server listening on {addr}")).then_some(()),
    )
    .await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// MB-R-256, MB-R-258 — with `reconnect` disabled the one failed bind attempt still logs an
/// Error naming the address, and the task ends.
async fn it_server_bind_failure_reconnect_false_logs_one_error() {
    use ferrowl_modbus::Level;
    let occupier = reserve_tcp_port();
    let port = occupier.port();
    let (log, lines) = capturing_levels();
    let (_sender, receiver) = mpsc::channel::<ServerCommand>(1);
    let (handle, _bound) = ferrowl_modbus::tcp::ServerBuilder::new(
        Arc::new(RwLock::new(tcp_config(port, false))),
        server_mem(),
        ferrowl_modbus::tcp::new_self_signed_cache(),
    )
    .spawn(receiver, log, sink())
    .await
    .expect("spawn always returns Ok");

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the task must end")
        .expect("task must not panic");
    assert!(result.is_err());
    let errors = lines
        .lock()
        .iter()
        .filter(|(l, s)| *l == Level::Error && s.contains(&format!("127.0.0.1:{port}")))
        .count();
    assert_eq!(errors, 1);
    drop(occupier);
}
