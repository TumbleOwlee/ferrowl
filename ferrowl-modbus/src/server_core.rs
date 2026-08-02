//! Transport-agnostic Modbus server request handler shared by the TCP and RTU servers.

use crate::{Key, KeyParams, LogFn};

use ferrowl_store::{CellType, Memory, Range};
use parking_lot::RwLock;
use rust_modbus::{
    Connection, ExceptionCode, FunctionCode, Quantity, RegisterValue, RequestPdu, ResponsePdu,
    Service, UnitId,
};
use std::fmt::Display;
use std::sync::Arc;

/// Shared body of the four read function codes: log the request, read `[addr, addr+cnt)` for the
/// `(slave, fc)` key as `cell`, log the outcome when `verbose`, and return the raw words. The
/// `name` is the only thing that varies between the read arms (and appears verbatim in the logs).
#[allow(clippy::too_many_arguments)] // request context (name/slave/fc/cell/addr/cnt) + server state
async fn handle_read<T, L>(
    name: &str,
    slave: UnitId,
    fc: FunctionCode,
    cell: CellType,
    addr: u16,
    cnt: u16,
    memory: &Arc<RwLock<Memory<Key<T>>>>,
    log: &L,
    verbose: bool,
) -> Result<Vec<u16>, ExceptionCode>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    log.invoke(format!(
        "{name} request received for slave ID {slave} and range [{}, {}).",
        addr,
        addr as usize + cnt as usize
    ))
    .await;
    let key = Key {
        id: T::from_slave_fn(slave, fc),
    };
    // Scoped so the (sync) guard is dropped before any log `.await` below.
    let result = {
        let guard = memory.read();
        guard.read(key, &cell, &Range::new(addr as usize, cnt as usize))
    };
    match result {
        Ok(v) => {
            if verbose {
                log.invoke(format!(
                    "{name} request for slave ID {slave} and range [{}, {}) successful.",
                    addr,
                    addr as usize + cnt as usize
                ))
                .await;
            }
            Ok(v)
        }
        Err(e) => {
            if verbose {
                log.invoke(format!(
                    "{name} request for slave ID {slave} and range [{}, {}) failed: {e}.",
                    addr,
                    addr as usize + cnt as usize
                ))
                .await;
            }
            Err(ExceptionCode::IllegalDataAddress)
        }
    }
}

/// Shared body of the two multi-write function codes (registers/coils): write `values` at `addr`
/// for the `(slave, fc)` key as `cell`, log the outcome, and return the count written (for the
/// response). Coil callers pass their bits already widened to `u16`.
#[allow(clippy::too_many_arguments)] // request context (name/slave/fc/cell/addr/values) + server state
async fn handle_write_multi<T, L>(
    name: &str,
    slave: UnitId,
    fc: FunctionCode,
    cell: CellType,
    addr: u16,
    values: &[u16],
    memory: &Arc<RwLock<Memory<Key<T>>>>,
    log: &L,
    verbose: bool,
) -> Result<u16, ExceptionCode>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    log.invoke(format!(
        "{name} request received for slave ID {slave}, range [{}, {}), and values {values:?}.",
        addr,
        addr as usize + values.len()
    ))
    .await;
    let key = Key {
        id: T::from_slave_fn(slave, fc),
    };
    // Scoped so the (sync) guard is dropped before any log `.await` below.
    let result = {
        let mut guard = memory.write();
        guard.write(key, &cell, &Range::new(addr as usize, values.len()), values)
    };
    match result {
        Ok(()) => {
            if verbose {
                log.invoke(format!(
                    "{name} request for slave ID {slave}, range [{}, {}), and values {values:?} successful.",
                    addr,
                    addr as usize + values.len()
                ))
                .await;
            }
            Ok(values.len() as u16)
        }
        Err(e) => {
            if verbose {
                log.invoke(format!(
                    "{name} request for slave ID {slave}, range [{}, {}), and values {values:?} failed: {e}.",
                    addr,
                    addr as usize + values.len()
                ))
                .await;
            }
            Err(ExceptionCode::IllegalDataAddress)
        }
    }
}

/// Shared body of the two single-write function codes (register/coil): write `stored` at `addr`
/// for the `(slave, fc)` key as `cell`, logging `value` (the protocol-level value — a `u16` for a
/// register, a `bool` for a coil) so the log text matches the wire request.
#[allow(clippy::too_many_arguments)] // request context (name/slave/fc/cell/addr/value/stored) + server state
async fn handle_write_single<T, L, V>(
    name: &str,
    slave: UnitId,
    fc: FunctionCode,
    cell: CellType,
    addr: u16,
    value: V,
    stored: u16,
    memory: &Arc<RwLock<Memory<Key<T>>>>,
    log: &L,
    verbose: bool,
) -> Result<(), ExceptionCode>
where
    T: KeyParams,
    L: LogFn + Clone,
    V: Display,
{
    log.invoke(format!(
        "{name} request received for slave ID {slave}, address {addr}, and value {value}."
    ))
    .await;
    let key = Key {
        id: T::from_slave_fn(slave, fc),
    };
    // Scoped so the (sync) guard is dropped before any log `.await` below.
    let result = {
        let mut guard = memory.write();
        guard.write(key, &cell, &Range::new(addr as usize, 1), &[stored])
    };
    match result {
        Ok(()) => {
            if verbose {
                log.invoke(format!(
                    "{name} request for slave ID {slave}, address {addr}, and value {value} successful."
                ))
                .await;
            }
            Ok(())
        }
        Err(e) => {
            if verbose {
                log.invoke(format!(
                    "{name} request for slave ID {slave}, address {addr}, and value {value} failed: {e}."
                ))
                .await;
            }
            Err(ExceptionCode::IllegalDataAddress)
        }
    }
}

/// Handle one inbound Modbus server request against `memory`, shared by the TCP and RTU servers.
///
/// Every arm logs a "request received" line. When `verbose` is set (TCP), each arm additionally
/// logs per-request success/failure; RTU passes `verbose = false` and stays quiet on the outcome.
pub(crate) async fn handle_request<T, L>(
    slave: UnitId,
    request: RequestPdu,
    memory: &Arc<RwLock<Memory<Key<T>>>>,
    log: &L,
    verbose: bool,
) -> Result<ResponsePdu, ExceptionCode>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    match request {
        RequestPdu::ReadCoils { address, quantity } => {
            let v = handle_read(
                "ReadCoils",
                slave,
                FunctionCode::ReadCoils,
                CellType::Coil,
                address.0,
                quantity.0,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::ReadCoils {
                coils: v.into_iter().map(|b| b != 0).collect(),
            })
        }
        RequestPdu::ReadDiscreteInputs { address, quantity } => {
            let v = handle_read(
                "ReadDiscreteInputs",
                slave,
                FunctionCode::ReadDiscreteInputs,
                CellType::Coil,
                address.0,
                quantity.0,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::ReadDiscreteInputs {
                inputs: v.into_iter().map(|b| b != 0).collect(),
            })
        }
        RequestPdu::ReadInputRegisters { address, quantity } => {
            let v = handle_read(
                "ReadInputRegisters",
                slave,
                FunctionCode::ReadInputRegisters,
                CellType::Register,
                address.0,
                quantity.0,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::ReadInputRegisters {
                registers: v.into_iter().map(RegisterValue).collect(),
            })
        }
        RequestPdu::ReadHoldingRegisters { address, quantity } => {
            let v = handle_read(
                "ReadHoldingRegisters",
                slave,
                FunctionCode::ReadHoldingRegisters,
                CellType::Register,
                address.0,
                quantity.0,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::ReadHoldingRegisters {
                registers: v.into_iter().map(RegisterValue).collect(),
            })
        }
        RequestPdu::WriteMultipleRegisters { address, registers } => {
            let values: Vec<u16> = registers.iter().map(|v| v.0).collect();
            let len = handle_write_multi(
                "WriteMultipleRegisters",
                slave,
                FunctionCode::WriteMultipleRegisters,
                CellType::Register,
                address.0,
                &values,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::WriteMultipleRegisters {
                address,
                quantity: Quantity(len),
            })
        }
        RequestPdu::WriteSingleRegister { address, value } => {
            handle_write_single(
                "WriteSingleRegister",
                slave,
                FunctionCode::WriteSingleRegister,
                CellType::Register,
                address.0,
                value.0,
                value.0,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::WriteSingleRegister { address, value })
        }
        RequestPdu::WriteMultipleCoils { address, coils } => {
            let values: Vec<u16> = coils.iter().map(|v| *v as u16).collect();
            let len = handle_write_multi(
                "WriteMultipleCoils",
                slave,
                FunctionCode::WriteMultipleCoils,
                CellType::Coil,
                address.0,
                &values,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::WriteMultipleCoils {
                address,
                quantity: Quantity(len),
            })
        }
        RequestPdu::WriteSingleCoil { address, value } => {
            handle_write_single(
                "WriteSingleCoil",
                slave,
                FunctionCode::WriteSingleCoil,
                CellType::Coil,
                address.0,
                value,
                value as u16,
                memory,
                log,
                verbose,
            )
            .await?;
            Ok(ResponsePdu::WriteSingleCoil { address, value })
        }
        RequestPdu::ReadWriteMultipleRegisters {
            read_address,
            read_quantity,
            write_address,
            registers,
        } => {
            let (read_addr, cnt, write_addr) = (read_address.0, read_quantity.0, write_address.0);
            let values: Vec<u16> = registers.iter().map(|v| v.0).collect();
            log.invoke(format!(
                "ReadWriteMultipleRegisrters request received for slave ID {}, read address {}, count {}, write address {}, and values {:?}.",
                slave, read_addr, cnt, write_addr, values
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::ReadWriteMultipleRegisters),
            };
            // The four checks/ops below must be atomic against concurrent requests, so they all
            // run under one scoped (sync) guard; it's dropped before any log `.await`.
            enum Outcome {
                NotAddressable(ferrowl_store::MemoryError),
                Rejected(ferrowl_store::MemoryError),
                Ok(Vec<u16>),
            }
            let outcome = {
                let mut guard = memory.write();
                match guard.readable(
                    &key,
                    &CellType::Register,
                    &Range::new(read_addr as usize, cnt as usize),
                ) {
                    Err(e) => Outcome::NotAddressable(e),
                    Ok(()) => match guard.writable(
                        &key,
                        &CellType::Register,
                        &Range::new(write_addr as usize, values.len()),
                    ) {
                        Err(e) => Outcome::NotAddressable(e),
                        Ok(()) => match guard.read(
                            key.clone(),
                            &CellType::Register,
                            &Range::new(read_addr as usize, cnt as usize),
                        ) {
                            Err(e) => Outcome::Rejected(e),
                            Ok(v) => match guard.write(
                                key,
                                &CellType::Register,
                                &Range::new(write_addr as usize, values.len()),
                                &values,
                            ) {
                                Err(e) => Outcome::Rejected(e),
                                Ok(()) => Outcome::Ok(v),
                            },
                        },
                    },
                }
            };
            match outcome {
                Outcome::NotAddressable(e) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} failed: {e}.",
                            slave, read_addr, cnt, write_addr, values
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalDataAddress)
                }
                Outcome::Rejected(e) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} failed: {e}.",
                            slave, read_addr, cnt, write_addr, values
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalDataAddress)
                }
                Outcome::Ok(v) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} successful.",
                            slave, read_addr, cnt, write_addr, values
                        ))
                        .await;
                    }
                    Ok(ResponsePdu::ReadWriteMultipleRegisters {
                        registers: v.into_iter().map(RegisterValue).collect(),
                    })
                }
            }
        }
        // MB-R-059: everything the server does not implement — report-server-id,
        // mask-write-register, diagnostics, comm-event, file record, FIFO queue,
        // read-device-identification (MEI), and any custom code — is one refusal, so a
        // function code the frame layer learns to decode later cannot silently acquire a
        // different answer here. MB-R-066 still requires the received line.
        other => {
            log.invoke(format!(
                "{} request received for slave ID {slave}. Unsupported function.",
                other.function(),
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
    }
}

/// Per-connection Modbus server service shared by the TCP and RTU servers: every request is
/// answered directly from the shared `memory` via [`handle_request`]. `verbose` toggles the
/// per-request success/failure logging — TCP sets it, RTU leaves it off. (The transport-specific
/// bind/accept vs serial-open setup stays in `tcp::server`/`rtu::server`.)
pub(crate) struct Server<T, L>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    memory: Arc<RwLock<Memory<Key<T>>>>,
    log: L,
    verbose: bool,
}

impl<T, L> Server<T, L>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    pub(crate) fn new(memory: Arc<RwLock<Memory<Key<T>>>>, log: L, verbose: bool) -> Self {
        Self {
            memory,
            log,
            verbose,
        }
    }
}

impl<T, L> Service for Server<T, L>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    // Taken by `&self` and awaited inside the connection's own task, so the handler suspends
    // normally on the store's lock and the log sink rather than blocking a worker thread. The
    // connection itself is not part of the answer: every request is served from the shared
    // store, whichever link carried it (MB-R-057).
    async fn on_request(
        &self,
        _conn: &Connection,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<ResponsePdu, ExceptionCode> {
        handle_request(unit, request, &self.memory, &self.log, self.verbose).await
    }

    // A framing or I/O failure on the wire never reaches `on_request`. TCP reports it (the
    // former `on_process_error` callback); RTU stays quiet on per-request outcomes, and
    // `verbose` is exactly that distinction (MB-R-067).
    async fn on_error(&self, _conn: &Connection, error: &rust_modbus::Error) {
        if self.verbose {
            self.log
                .invoke(format!("Server processing failed. [{error}]"))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SlaveKey;
    use ferrowl_codec::Kind as RegKind;
    use ferrowl_store::CellKind as MemKind;
    use rust_modbus::{
        Address, Client as RmClient, DiagnosticSubFunction, FrameTransport, Mask, MeiRequest,
        ReadDeviceIdCode, Server as ModbusServer, Tcp,
    };
    use std::sync::Mutex;

    /// Build a memory map for slave `1`, holding registers `[0,4)`, seeded with `seed` at addr 0,
    /// wrapped in the `Arc<RwLock<_>>` that `handle_request` expects.
    fn seeded_memory(seed: &[u16]) -> Arc<RwLock<Memory<Key<SlaveKey>>>> {
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: RegKind::HoldingRegister,
            },
        };
        let mut mem = Memory::<Key<SlaveKey>>::default();
        mem.add_ranges(
            key.clone(),
            &MemKind::ReadWrite(CellType::Register),
            &[Range::new(0, 4)],
        );
        if !seed.is_empty() {
            mem.write(key, &CellType::Register, &Range::new(0, seed.len()), seed)
                .unwrap();
        }
        Arc::new(RwLock::new(mem))
    }

    /// A `LogFn` that records every line into a shared buffer for assertions.
    fn recording_log() -> (impl LogFn + Clone, Arc<Mutex<Vec<String>>>) {
        let buf = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = buf.clone();
        let log = move |s: String| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(s);
            }
        };
        (log, buf)
    }

    // Regression: the service used to bridge into async via `block_in_place` +
    // `Handle::block_on` purely to lock `memory`, which panics ("can call blocking only when
    // running on the multi-threaded runtime") on the default current-thread flavor below. Now
    // that the lock is synchronous (`parking_lot`) and `on_request` is an ordinary async fn the
    // connection task just `.await`s, this must succeed on a current-thread runtime with no
    // dedicated worker threads to bridge onto.
    #[tokio::test]
    /// MB-R-057 — the server answers an inbound request directly from the shared store.
    async fn ut_server_call_works_on_current_thread_runtime() {
        let mem = seeded_memory(&[10, 20]);
        let (log, _) = recording_log();
        let server = Server::new(mem, log, true);

        // Served over an in-memory duplex link, which is the same code path a socket takes:
        // the accept loop is the only thing a real listener adds.
        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(server);
        let handle = modbus.handle();
        let serving = tokio::spawn(modbus.serve_link(FrameTransport::<_, Tcp>::new(server_end)));

        let mut client: RmClient<_, Tcp> = RmClient::new(FrameTransport::new(client_end));
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(2))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(10), RegisterValue(20)]);

        handle.shutdown().await;
        let _ = serving.await;
    }

    #[tokio::test]
    /// MB-R-057 — a holding-register read is answered from the values stored in the shared store.
    async fn ut_handle_read_holding_returns_seeded_values() {
        let mem = seeded_memory(&[10, 20]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            true,
        )
        .await
        .unwrap();
        assert!(
            matches!(resp, ResponsePdu::ReadHoldingRegisters { registers: v } if v == vec![RegisterValue(10), RegisterValue(20)])
        );
    }

    #[tokio::test]
    /// MB-R-060 — a read against a slave with no declared regions is answered with `IllegalDataAddress`.
    async fn ut_handle_read_unknown_slave_is_illegal_data_address() {
        let mem = seeded_memory(&[10, 20]);
        let (log, _) = recording_log();
        // Slave 2 has no registered ranges, so the lookup fails.
        let err = handle_request::<SlaveKey, _>(
            UnitId(2),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    #[tokio::test]
    /// MB-R-058 — a write-single-register request is answered and its value persisted in the store.
    async fn ut_handle_write_single_register_persists() {
        let mem = seeded_memory(&[]);
        let (log, _) = recording_log();
        handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteSingleRegister {
                address: Address(1),
                value: RegisterValue(99),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(1),
                quantity: Quantity(1),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(resp, ResponsePdu::ReadHoldingRegisters { registers: v } if v == vec![RegisterValue(99)])
        );
    }

    #[tokio::test]
    /// MB-R-067 — a TCP (verbose) server logs the per-request outcome; without verbose only the "received" line.
    async fn ut_handle_verbose_logs_outcome_quiet_when_off() {
        let mem = seeded_memory(&[1, 2]);

        // verbose = true: a "received" line plus a "successful" line.
        let (log, buf) = recording_log();
        handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            true,
        )
        .await
        .unwrap();
        let verbose = buf.lock().unwrap().clone();
        assert_eq!(verbose.len(), 2);
        assert!(verbose[0].contains("received"));
        assert!(verbose[1].contains("successful"));

        // verbose = false: only the "received" line.
        let (log, buf) = recording_log();
        handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        let quiet = buf.lock().unwrap().clone();
        assert_eq!(quiet.len(), 1);
        assert!(quiet[0].contains("received"));
    }

    /// Build a memory for slave `1` of the given register `kind`/value `ty` over `[0, len)`,
    /// optionally seeded with `seed` at addr 0. ReadWrite so both read and write paths work.
    fn seeded(
        kind: RegKind,
        ty: CellType,
        len: usize,
        seed: &[u16],
    ) -> Arc<RwLock<Memory<Key<SlaveKey>>>> {
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind,
            },
        };
        let mut mem = Memory::<Key<SlaveKey>>::default();
        mem.add_ranges(key.clone(), &MemKind::ReadWrite(ty), &[Range::new(0, len)]);
        if !seed.is_empty() {
            mem.write(key, &ty, &Range::new(0, seed.len()), seed)
                .unwrap();
        }
        Arc::new(RwLock::new(mem))
    }

    // ---- WriteMultipleCoils: regression for the hard-coded range length bug ----

    #[tokio::test]
    /// MB-R-062 — a multi-coil write is answered with the address written and the number of values written.
    async fn ut_write_multiple_coils_persists_every_bit() {
        let mem = seeded(RegKind::Coil, CellType::Coil, 8, &[]);
        let (log, _) = recording_log();
        let coils = vec![true, false, true, true, false];

        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteMultipleCoils {
                address: Address(1),
                coils: coils.clone(),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        // Regression: the write range length must equal values.len(), not 1. Before the fix
        // `Memory::write` rejected any multi-coil write (range.length() != values.len()).
        assert!(matches!(
            resp,
            ResponsePdu::WriteMultipleCoils {
                address: Address(1),
                quantity: Quantity(5)
            }
        ));

        let read = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadCoils {
                address: Address(1),
                quantity: Quantity(5),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(read, ResponsePdu::ReadCoils { coils: v } if v == coils));
    }

    #[tokio::test]
    /// MB-R-060 — a multi-coil write overrunning the declared region is answered with `IllegalDataAddress`.
    async fn ut_write_multiple_coils_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::Coil, CellType::Coil, 8, &[]);
        let (log, _) = recording_log();
        // addr 6 + 5 coils overruns the registered [0, 8) region.
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteMultipleCoils {
                address: Address(6),
                coils: vec![true; 5],
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    // ---- Coil / discrete-input reads ----

    #[tokio::test]
    /// MB-R-061 — a coil read reports each stored word as set when it is non-zero.
    async fn ut_read_coils_returns_seeded_bits() {
        let mem = seeded(RegKind::Coil, CellType::Coil, 4, &[1, 0, 1, 0]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadCoils {
                address: Address(0),
                quantity: Quantity(4),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(resp, ResponsePdu::ReadCoils { coils: v } if v == vec![true, false, true, false])
        );
    }

    #[tokio::test]
    /// MB-R-060 — a coil read outside the declared region is answered with `IllegalDataAddress`.
    async fn ut_read_coils_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::Coil, CellType::Coil, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadCoils {
                address: Address(10),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    #[tokio::test]
    /// MB-R-058 — the server answers read-discrete-inputs from the stored bits.
    async fn ut_read_discrete_inputs_returns_seeded_bits() {
        let mem = seeded(RegKind::DiscreteInput, CellType::Coil, 3, &[0, 1, 1]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadDiscreteInputs {
                address: Address(0),
                quantity: Quantity(3),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(resp, ResponsePdu::ReadDiscreteInputs { inputs: v } if v == vec![false, true, true])
        );
    }

    #[tokio::test]
    /// MB-R-060 — a discrete-input read against an undeclared slave is answered with `IllegalDataAddress`.
    async fn ut_read_discrete_inputs_unknown_slave_is_illegal_data_address() {
        let mem = seeded(RegKind::DiscreteInput, CellType::Coil, 3, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(2),
            RequestPdu::ReadDiscreteInputs {
                address: Address(0),
                quantity: Quantity(3),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    // ---- Register reads ----

    #[tokio::test]
    /// MB-R-057 — an input-register read is answered from the values stored in the shared store.
    async fn ut_read_input_registers_returns_seeded_values() {
        let mem = seeded(RegKind::InputRegister, CellType::Register, 3, &[7, 8, 9]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadInputRegisters {
                address: Address(0),
                quantity: Quantity(3),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(resp, ResponsePdu::ReadInputRegisters { registers: v } if v == vec![RegisterValue(7), RegisterValue(8), RegisterValue(9)])
        );
    }

    #[tokio::test]
    /// MB-R-060 — an input-register read outside the declared region is answered with `IllegalDataAddress`.
    async fn ut_read_input_registers_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::InputRegister, CellType::Register, 3, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadInputRegisters {
                address: Address(2),
                quantity: Quantity(5),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    #[tokio::test]
    /// MB-R-060 — a holding-register read outside the declared region is answered with `IllegalDataAddress`.
    async fn ut_read_holding_registers_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(3),
                quantity: Quantity(4),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    // ---- Single writes ----

    #[tokio::test]
    /// MB-R-061 — a coil write stores a set coil, observable on read-back.
    async fn ut_write_single_coil_persists() {
        let mem = seeded(RegKind::Coil, CellType::Coil, 4, &[]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteSingleCoil {
                address: Address(2),
                value: true,
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(
            resp,
            ResponsePdu::WriteSingleCoil {
                address: Address(2),
                value: true
            }
        ));
        let read = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadCoils {
                address: Address(2),
                quantity: Quantity(1),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(read, ResponsePdu::ReadCoils { coils: v } if v == vec![true]));
    }

    #[tokio::test]
    /// MB-R-060 — a single-coil write outside the declared region is answered with `IllegalDataAddress`.
    async fn ut_write_single_coil_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::Coil, CellType::Coil, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteSingleCoil {
                address: Address(9),
                value: true,
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    #[tokio::test]
    /// MB-R-060 — a single-register write outside the declared region is answered with `IllegalDataAddress`.
    async fn ut_write_single_register_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteSingleRegister {
                address: Address(99),
                value: RegisterValue(1),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    // ---- WriteMultipleRegisters ----

    #[tokio::test]
    /// MB-R-062 — a multi-register write is answered with the address written and the number of values written.
    async fn ut_write_multiple_registers_persists_all() {
        let mem = seeded(RegKind::HoldingRegister, CellType::Register, 8, &[]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteMultipleRegisters {
                address: Address(1),
                registers: vec![11, 22, 33].into_iter().map(RegisterValue).collect(),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(
            resp,
            ResponsePdu::WriteMultipleRegisters {
                address: Address(1),
                quantity: Quantity(3)
            }
        ));
        let read = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(1),
                quantity: Quantity(3),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(read, ResponsePdu::ReadHoldingRegisters { registers: v } if v == vec![RegisterValue(11), RegisterValue(22), RegisterValue(33)])
        );
    }

    #[tokio::test]
    /// MB-R-060 — a multi-register write outside the declared region is answered with `IllegalDataAddress`.
    async fn ut_write_multiple_registers_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::WriteMultipleRegisters {
                address: Address(3),
                registers: vec![1, 2, 3].into_iter().map(RegisterValue).collect(),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    // ---- ReadWriteMultipleRegisters (reads and writes the same holding region) ----

    #[tokio::test]
    /// MB-R-063 — a read/write-multiple request applies the write and returns the values read before it.
    async fn ut_read_write_multiple_registers_writes_then_returns_read() {
        let mem = seeded(
            RegKind::HoldingRegister,
            CellType::Register,
            8,
            &[5, 6, 7, 8],
        );
        let (log, _) = recording_log();
        // Read [0,2), write [2,4) = [77, 88].
        let resp = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadWriteMultipleRegisters {
                read_address: Address(0),
                read_quantity: Quantity(2),
                write_address: Address(2),
                registers: vec![77, 88].into_iter().map(RegisterValue).collect(),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(resp, ResponsePdu::ReadWriteMultipleRegisters { registers: v } if v == vec![RegisterValue(5), RegisterValue(6)])
        );
        let read = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(2),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(read, ResponsePdu::ReadHoldingRegisters { registers: v } if v == vec![RegisterValue(77), RegisterValue(88)])
        );
    }

    #[tokio::test]
    /// MB-R-064 — a read/write-multiple whose write range is not writable is answered `IllegalDataAddress` and applies no write.
    async fn ut_read_write_multiple_registers_out_of_range_is_illegal_data_address() {
        let mem = seeded(
            RegKind::HoldingRegister,
            CellType::Register,
            4,
            &[1, 2, 3, 4],
        );
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadWriteMultipleRegisters {
                read_address: Address(0),
                read_quantity: Quantity(2),
                write_address: Address(10),
                registers: vec![1, 2].into_iter().map(RegisterValue).collect(),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    #[tokio::test]
    /// MB-R-065 — the server answers any slave id for which regions are declared; it does not filter by a configured slave id.
    async fn ut_server_serves_any_declared_slave() {
        // Regions declared for two different slave ids; the server has no "configured" slave.
        let mut mem = Memory::<Key<SlaveKey>>::default();
        for &slave in &[3u8, 9u8] {
            let key = Key {
                id: SlaveKey {
                    slave_id: UnitId(slave),
                    kind: RegKind::HoldingRegister,
                },
            };
            mem.add_ranges(
                key.clone(),
                &MemKind::ReadWrite(CellType::Register),
                &[Range::new(0, 2)],
            );
            mem.write(
                key,
                &CellType::Register,
                &Range::new(0, 2),
                &[slave as u16, 0],
            )
            .unwrap();
        }
        let mem = Arc::new(RwLock::new(mem));
        let (log, _) = recording_log();

        // Both slaves are answered from their own declared regions.
        for &slave in &[3u8, 9u8] {
            let resp = handle_request::<SlaveKey, _>(
                UnitId(slave),
                RequestPdu::ReadHoldingRegisters {
                    address: Address(0),
                    quantity: Quantity(2),
                },
                &mem,
                &log,
                false,
            )
            .await
            .unwrap();
            assert!(
                matches!(resp, ResponsePdu::ReadHoldingRegisters { registers: v } if v[0] == RegisterValue(slave as u16))
            );
        }
    }

    #[tokio::test]
    /// MB-R-066 — a "request received" line is logged for every inbound request, including a rejected function code.
    async fn ut_logs_request_received_including_rejected() {
        let mem = seeded(
            RegKind::HoldingRegister,
            CellType::Register,
            4,
            &[10, 20, 30, 40],
        );

        // A supported request logs "request received".
        let (log, buf) = recording_log();
        handle_request::<SlaveKey, _>(
            UnitId(1),
            RequestPdu::ReadHoldingRegisters {
                address: Address(0),
                quantity: Quantity(2),
            },
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(
            buf.lock()
                .unwrap()
                .iter()
                .any(|l| l.contains("request received"))
        );

        // A rejected function code still logs "request received" before the IllegalFunction reply.
        let (log, buf) = recording_log();
        let err =
            handle_request::<SlaveKey, _>(UnitId(1), RequestPdu::ReportServerId, &mem, &log, false)
                .await
                .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
        assert!(
            buf.lock()
                .unwrap()
                .iter()
                .any(|l| l.contains("request received"))
        );
    }

    // ---- Unsupported function codes ----

    #[tokio::test]
    /// MB-R-059 — report-server-id is rejected with `IllegalFunction`.
    async fn ut_report_server_id_is_illegal_function() {
        let mem = seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]);
        let (log, _) = recording_log();
        let err =
            handle_request::<SlaveKey, _>(UnitId(1), RequestPdu::ReportServerId, &mem, &log, false)
                .await
                .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- Verbose logging: every arm's success/failure log branch ----

    #[tokio::test]
    /// MB-R-067 — a TCP (verbose) server logs a success outcome for every supported request.
    async fn ut_verbose_logs_success_for_every_request() {
        macro_rules! ok {
            ($mem:expr, $req:expr) => {{
                let mem = $mem;
                let (log, buf) = recording_log();
                handle_request::<SlaveKey, _>(UnitId(1), $req, &mem, &log, true)
                    .await
                    .unwrap();
                assert!(
                    buf.lock().unwrap().iter().any(|l| l.contains("successful")),
                    "missing success log line"
                );
            }};
        }
        ok!(
            seeded(RegKind::Coil, CellType::Coil, 8, &[1, 0, 1, 0, 1, 0, 1, 0]),
            RequestPdu::ReadCoils {
                address: Address(0),
                quantity: Quantity(4)
            }
        );
        ok!(
            seeded(RegKind::Coil, CellType::Coil, 8, &[]),
            RequestPdu::WriteSingleCoil {
                address: Address(0),
                value: true
            }
        );
        ok!(
            seeded(RegKind::Coil, CellType::Coil, 8, &[]),
            RequestPdu::WriteMultipleCoils {
                address: Address(0),
                coils: vec![true, false, true]
            }
        );
        ok!(
            seeded(RegKind::DiscreteInput, CellType::Coil, 4, &[1, 1, 1, 1]),
            RequestPdu::ReadDiscreteInputs {
                address: Address(0),
                quantity: Quantity(4)
            }
        );
        ok!(
            seeded(RegKind::InputRegister, CellType::Register, 4, &[1, 2, 3, 4]),
            RequestPdu::ReadInputRegisters {
                address: Address(0),
                quantity: Quantity(4)
            }
        );
        ok!(
            seeded(RegKind::HoldingRegister, CellType::Register, 8, &[]),
            RequestPdu::WriteSingleRegister {
                address: Address(0),
                value: RegisterValue(9)
            }
        );
        ok!(
            seeded(RegKind::HoldingRegister, CellType::Register, 8, &[]),
            RequestPdu::WriteMultipleRegisters {
                address: Address(0),
                registers: vec![1, 2, 3].into_iter().map(RegisterValue).collect()
            }
        );
        ok!(
            seeded(
                RegKind::HoldingRegister,
                CellType::Register,
                8,
                &[5, 6, 7, 8]
            ),
            RequestPdu::ReadWriteMultipleRegisters {
                read_address: Address(0),
                read_quantity: Quantity(2),
                write_address: Address(2),
                registers: vec![7, 8].into_iter().map(RegisterValue).collect()
            }
        );
    }

    #[tokio::test]
    /// MB-R-067 — a TCP (verbose) server logs a failure outcome for every rejected request.
    async fn ut_verbose_logs_failure_for_every_request() {
        macro_rules! fail {
            ($mem:expr, $req:expr) => {{
                let mem = $mem;
                let (log, buf) = recording_log();
                let _ = handle_request::<SlaveKey, _>(UnitId(1), $req, &mem, &log, true).await;
                assert!(
                    buf.lock().unwrap().iter().any(|l| l.contains("failed")),
                    "missing failure log line"
                );
            }};
        }
        fail!(
            seeded(RegKind::Coil, CellType::Coil, 4, &[]),
            RequestPdu::ReadCoils {
                address: Address(10),
                quantity: Quantity(2)
            }
        );
        fail!(
            seeded(RegKind::Coil, CellType::Coil, 4, &[]),
            RequestPdu::WriteSingleCoil {
                address: Address(9),
                value: true
            }
        );
        fail!(
            seeded(RegKind::Coil, CellType::Coil, 4, &[]),
            RequestPdu::WriteMultipleCoils {
                address: Address(6),
                coils: vec![true; 5]
            }
        );
        fail!(
            seeded(RegKind::DiscreteInput, CellType::Coil, 4, &[]),
            RequestPdu::ReadDiscreteInputs {
                address: Address(10),
                quantity: Quantity(2)
            }
        );
        fail!(
            seeded(RegKind::InputRegister, CellType::Register, 4, &[]),
            RequestPdu::ReadInputRegisters {
                address: Address(10),
                quantity: Quantity(2)
            }
        );
        fail!(
            seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]),
            RequestPdu::ReadHoldingRegisters {
                address: Address(10),
                quantity: Quantity(2)
            }
        );
        fail!(
            seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]),
            RequestPdu::WriteSingleRegister {
                address: Address(99),
                value: RegisterValue(1)
            }
        );
        fail!(
            seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]),
            RequestPdu::WriteMultipleRegisters {
                address: Address(3),
                registers: vec![1, 2, 3].into_iter().map(RegisterValue).collect()
            }
        );
        // Write address out of range -> writable check fails (verbose failure branch).
        fail!(
            seeded(
                RegKind::HoldingRegister,
                CellType::Register,
                4,
                &[1, 2, 3, 4]
            ),
            RequestPdu::ReadWriteMultipleRegisters {
                read_address: Address(0),
                read_quantity: Quantity(2),
                write_address: Address(10),
                registers: vec![1, 2].into_iter().map(RegisterValue).collect()
            }
        );
    }

    #[tokio::test]
    /// MB-R-059 — every function code outside the nine the server implements is rejected with
    /// `IllegalFunction`, whatever the frame layer is able to decode.
    async fn ut_unsupported_function_codes_are_illegal() {
        let mem = seeded(RegKind::HoldingRegister, CellType::Register, 4, &[]);
        let (log, _) = recording_log();
        for req in [
            RequestPdu::MaskWriteRegister {
                address: Address(0),
                and_mask: Mask(0),
                or_mask: Mask(0),
            },
            RequestPdu::EncapsulatedInterfaceTransport(MeiRequest::ReadDeviceIdentification {
                read_device_id_code: ReadDeviceIdCode::Basic,
                object_id: 0,
            }),
            RequestPdu::ReportServerId,
            RequestPdu::ReadExceptionStatus,
            RequestPdu::GetCommEventCounter,
            RequestPdu::GetCommEventLog,
            RequestPdu::Diagnostics {
                sub_function: DiagnosticSubFunction::ReturnQueryData,
                data: vec![0x1234],
            },
            RequestPdu::ReadFifoQueue {
                address: Address(0),
            },
            RequestPdu::Custom {
                code: 0x65,
                data: vec![],
            },
        ] {
            let err = handle_request::<SlaveKey, _>(UnitId(1), req, &mem, &log, false)
                .await
                .unwrap_err();
            assert_eq!(err, ExceptionCode::IllegalFunction);
        }
    }
}
