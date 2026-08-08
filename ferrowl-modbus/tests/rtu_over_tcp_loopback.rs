//! End-to-end RTU-over-TCP test: a ferrowl Modbus server and client speak RTU framing
//! (unit id + CRC, no MBAP header) over a loopback TCP socket. Reuses `tcp::Config` for
//! connection settings (MB-R-113) — only the on-wire framing differs from plain TCP.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::Kind as RegKind;
use ferrowl_modbus::tcp;
use ferrowl_modbus::{Address, Command, FunctionCode, Key, Operation, SlaveKey, UnitId, Word};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use parking_lot::RwLock as MemLock;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

type Mem = Arc<MemLock<Memory<Key<SlaveKey>>>>;

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
        tls: None,
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
/// MB-R-114 — RTU-over-TCP carries the same client/server behavior as plain TCP (polling every
/// read operation into the shared store, MB-R-035, executing write commands, MB-R-046, and
/// terminating gracefully, MB-R-049), differing only in on-wire framing.
async fn rtu_over_tcp_client_polls_server_and_executes_commands() {
    let port = free_port();
    let srv_mem = server_mem();
    let cli_mem = client_mem();

    // Start the server.
    let server = ferrowl_modbus::rtu_over_tcp::ServerBuilder::new(
        Arc::new(RwLock::new(config(port))),
        srv_mem.clone(),
    )
    .spawn(sink())
    .await
    .expect("server failed to start");

    // Operations cover every read function code the client supports.
    let operations = Arc::new(RwLock::new(vec![
        Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadCoils,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadDiscreteInputs,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadInputRegisters,
            range: Range::new(0, 4),
        },
        Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 4),
        },
    ]));

    let (tx, rx) = mpsc::channel::<Command>(16);
    let client = ferrowl_modbus::rtu_over_tcp::ClientBuilder::new(
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
    tx.send(Command::WriteSingleRegister(
        UnitId(1),
        Address(0),
        Word(99),
    ))
    .await
    .unwrap();
    tx.send(Command::WriteMultipleRegister(
        UnitId(1),
        Address(1),
        vec![5, 6].into_iter().map(Word).collect(),
    ))
    .await
    .unwrap();
    tx.send(Command::WriteSingleCoil(UnitId(1), Address(5), true))
        .await
        .unwrap();
    tx.send(Command::WriteMultipleCoils(
        UnitId(1),
        Address(6),
        vec![true, false],
    ))
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
