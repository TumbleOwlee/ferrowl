//! Transport-agnostic Modbus server request handler shared by the TCP and RTU servers.

use crate::{Key, KeyParams, LogFn, SlaveId};

use ferrowl_mem::{Memory, Range, Type};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_modbus::FunctionCode;
use tokio_modbus::Request;
use tokio_modbus::prelude::{ExceptionCode, Response};

/// Handle one inbound Modbus server request against `memory`, shared by the TCP and RTU servers.
///
/// Every arm logs a "request received" line. When `verbose` is set (TCP), each arm additionally
/// logs per-request success/failure; RTU passes `verbose = false` and stays quiet on the outcome.
pub(crate) async fn handle_request<T, L>(
    slave: SlaveId,
    request: Request<'static>,
    memory: &Arc<RwLock<Memory<Key<T>>>>,
    log: &L,
    verbose: bool,
) -> Result<Response, ExceptionCode>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    match request {
        Request::ReadCoils(addr, cnt) => {
            log.invoke(format!(
                "ReadCoils request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::ReadCoils),
            };
            let guard = memory.read().await;
            match guard.read(key, &Type::Coil, &Range::new(addr as usize, cnt as usize)) {
                Some(v) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadCoils request for slave ID {} and range [{}, {}) successful.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Ok(Response::ReadCoils(v.into_iter().map(|b| b != 0).collect()))
                }
                None => {
                    if verbose {
                        log.invoke(format!(
                            "ReadCoils request for slave ID {} and range [{}, {}) failed.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::ReadDiscreteInputs(addr, cnt) => {
            log.invoke(format!(
                "ReadDiscreteInputs request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::ReadDiscreteInputs),
            };
            let guard = memory.read().await;
            match guard.read(key, &Type::Coil, &Range::new(addr as usize, cnt as usize)) {
                Some(v) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadDiscreteInputs request for slave ID {} and range [{}, {}) successful.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Ok(Response::ReadDiscreteInputs(
                        v.into_iter().map(|b| b != 0).collect(),
                    ))
                }
                None => {
                    if verbose {
                        log.invoke(format!(
                            "ReadDiscreteInputs request for slave ID {} and range [{}, {}) failed.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::ReadInputRegisters(addr, cnt) => {
            log.invoke(format!(
                "ReadInputRegisters request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::ReadInputRegisters),
            };
            let guard = memory.read().await;
            match guard.read(
                key,
                &Type::Register,
                &Range::new(addr as usize, cnt as usize),
            ) {
                Some(v) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadInputRegisters request for slave ID {} and range [{}, {}) successful.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Ok(Response::ReadInputRegisters(v))
                }
                None => {
                    if verbose {
                        log.invoke(format!(
                            "ReadInputRegisters request for slave ID {} and range [{}, {}) failed.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::ReadHoldingRegisters(addr, cnt) => {
            log.invoke(format!(
                "ReadHoldingRegisters request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::ReadHoldingRegisters),
            };
            let guard = memory.read().await;
            match guard.read(
                key,
                &Type::Register,
                &Range::new(addr as usize, cnt as usize),
            ) {
                Some(v) => {
                    if verbose {
                        log.invoke(format!(
                            "ReadHoldingRegisters request for slave ID {} and range [{}, {}) successful.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Ok(Response::ReadHoldingRegisters(v))
                }
                None => {
                    if verbose {
                        log.invoke(format!(
                            "ReadHoldingRegisters request for slave ID {} and range [{}, {}) failed.",
                            slave,
                            addr,
                            addr + cnt
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::WriteMultipleRegisters(addr, values) => {
            log.invoke(format!(
                "WriteMultipleRegisters request received for slave ID {}, range [{}, {}), and values {:?}.",
                slave,
                addr,
                addr as usize + values.len(),
                values
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::WriteMultipleRegisters),
            };
            let mut guard = memory.write().await;
            match guard.write(
                key,
                &Type::Register,
                &Range::new(addr as usize, values.len()),
                &values,
            ) {
                true => {
                    if verbose {
                        log.invoke(format!(
                            "WriteMultipleRegisters request for slave ID {}, range [{}, {}), and values {:?} successful.",
                            slave,
                            addr,
                            addr as usize + values.len(),
                            values
                        ))
                        .await;
                    }
                    Ok(Response::WriteMultipleRegisters(addr, values.len() as u16))
                }
                false => {
                    if verbose {
                        log.invoke(format!(
                            "WriteMultipleRegisters request for slave ID {}, range [{}, {}), and values {:?} failed.",
                            slave,
                            addr,
                            addr as usize + values.len(),
                            values
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::WriteSingleRegister(addr, value) => {
            log.invoke(format!(
                "WriteSingleRegister request received for slave ID {}, address {}, and value {}.",
                slave, addr, value
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::WriteSingleRegister),
            };
            let mut guard = memory.write().await;
            match guard.write(
                key,
                &Type::Register,
                &Range::new(addr as usize, 1),
                &[value],
            ) {
                true => {
                    if verbose {
                        log.invoke(format!(
                            "WriteSingleRegister request for slave ID {}, address {}, and value {} successful.",
                            slave, addr, value
                        ))
                        .await;
                    }
                    Ok(Response::WriteSingleRegister(addr, value))
                }
                false => {
                    if verbose {
                        log.invoke(format!(
                            "WriteSingleRegister request for slave ID {}, address {}, and value {} failed.",
                            slave, addr, value
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::WriteMultipleCoils(addr, values) => {
            log.invoke(format!(
                "WriteMultipleCoils request received for slave ID {}, range [{}, {}), and values {:?}.",
                slave,
                addr,
                addr as usize + values.len(),
                values
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::WriteMultipleCoils),
            };
            let mut guard = memory.write().await;
            let values: Vec<u16> = values.iter().map(|v| *v as u16).collect();
            match guard.write(key, &Type::Coil, &Range::new(addr as usize, values.len()), &values) {
                true => {
                    if verbose {
                        log.invoke(format!(
                            "WriteMultipleCoils request for slave ID {}, range [{}, {}), and values {:?} successful.",
                            slave,
                            addr,
                            addr as usize + values.len(),
                            values
                        ))
                        .await;
                    }
                    Ok(Response::WriteMultipleCoils(addr, values.len() as u16))
                }
                false => {
                    if verbose {
                        log.invoke(format!(
                            "WriteMultipleCoils request for slave ID {}, range [{}, {}), and values {:?} failed.",
                            slave,
                            addr,
                            addr as usize + values.len(),
                            values
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::WriteSingleCoil(addr, value) => {
            log.invoke(format!(
                "WriteSingleCoil request received for slave ID {}, address {}, and value {}.",
                slave, addr, value
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::WriteSingleCoil),
            };
            let mut guard = memory.write().await;
            let val = value as u16;
            match guard.write(key, &Type::Coil, &Range::new(addr as usize, 1), &[val]) {
                true => {
                    if verbose {
                        log.invoke(format!(
                            "WriteSingleCoil request for slave ID {}, address {}, and value {} successful.",
                            slave, addr, value
                        ))
                        .await;
                    }
                    Ok(Response::WriteSingleCoil(addr, value))
                }
                false => {
                    if verbose {
                        log.invoke(format!(
                            "WriteSingleCoil request for slave ID {}, address {}, and value {} failed.",
                            slave, addr, value
                        ))
                        .await;
                    }
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }
        Request::ReportServerId => {
            log.invoke(format!(
                "ReportServerId request received for slave ID {}. Unsupported function.",
                slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
        Request::MaskWriteRegister(_, _, _) => {
            log.invoke(format!(
                "MaskWriteRegister request received for slave ID {}. Unsupported function.",
                slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
        Request::ReadWriteMultipleRegisters(read_addr, cnt, write_addr, values) => {
            log.invoke(format!(
                "ReadWriteMultipleRegisrters request received for slave ID {}, read address {}, count {}, write address {}, and values {:?}.",
                slave, read_addr, cnt, write_addr, values
            ))
            .await;
            let key = Key {
                id: T::from_slave_fn(slave, FunctionCode::ReadWriteMultipleRegisters),
            };
            let mut guard = memory.write().await;
            let readable = guard.readable(
                &key,
                &Type::Register,
                &Range::new(read_addr as usize, cnt as usize),
            );
            let writable = guard.writable(
                &key,
                &Type::Register,
                &Range::new(write_addr as usize, values.len()),
            );
            if !readable || !writable {
                if verbose {
                    log.invoke(format!(
                        "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} failed.",
                        slave, read_addr, cnt, write_addr, values
                    ))
                    .await;
                }
                return Err(ExceptionCode::IllegalDataAddress);
            }
            let v = match guard.read(
                key.clone(),
                &Type::Register,
                &Range::new(read_addr as usize, cnt as usize),
            ) {
                Some(v) => v,
                None => {
                    if verbose {
                        log.invoke(format!(
                            "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} failed.",
                            slave, read_addr, cnt, write_addr, values
                        ))
                        .await;
                    }
                    return Err(ExceptionCode::IllegalFunction);
                }
            };
            if !guard.write(
                key,
                &Type::Register,
                &Range::new(write_addr as usize, values.len()),
                &values,
            ) {
                if verbose {
                    log.invoke(format!(
                        "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} failed.",
                        slave, read_addr, cnt, write_addr, values
                    ))
                    .await;
                }
                return Err(ExceptionCode::IllegalFunction);
            }
            if verbose {
                log.invoke(format!(
                    "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} successful.",
                    slave, read_addr, cnt, write_addr, values
                ))
                .await;
            }
            Ok(Response::ReadWriteMultipleRegisters(v))
        }
        Request::ReadDeviceIdentification(_, _) => {
            log.invoke(format!(
                "ReadDeviceIdentification request received for slave ID {}. Unsupported function.",
                slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
        Request::Custom(func, _) => {
            log.invoke(format!(
                "Custom function {} request received for slave ID {}. Unsupported function.",
                func, slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SlaveKind;
    use ferrowl_mem::Kind as MemKind;
    use ferrowl_reg::Kind as RegKind;
    use std::sync::Mutex;

    /// Build a memory map for slave `1`, holding registers `[0,4)`, seeded with `seed` at addr 0,
    /// wrapped in the `Arc<RwLock<_>>` that `handle_request` expects.
    fn seeded_memory(seed: &[u16]) -> Arc<RwLock<Memory<Key<SlaveKind>>>> {
        let key = Key {
            id: SlaveKind {
                slave_id: 1,
                kind: RegKind::HoldingRegister,
            },
        };
        let mut mem = Memory::<Key<SlaveKind>>::default();
        mem.add_ranges(
            key.clone(),
            &MemKind::ReadWrite(Type::Register),
            &[Range::new(0, 4)],
        );
        if !seed.is_empty() {
            mem.write(key, &Type::Register, &Range::new(0, seed.len()), seed);
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

    #[tokio::test]
    async fn ut_handle_read_holding_returns_seeded_values() {
        let mem = seeded_memory(&[10, 20]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKind, _>(
            1,
            Request::ReadHoldingRegisters(0, 2),
            &mem,
            &log,
            true,
        )
        .await
        .unwrap();
        assert!(matches!(resp, Response::ReadHoldingRegisters(v) if v == vec![10, 20]));
    }

    #[tokio::test]
    async fn ut_handle_read_unknown_slave_is_illegal_function() {
        let mem = seeded_memory(&[10, 20]);
        let (log, _) = recording_log();
        // Slave 2 has no registered ranges, so the lookup fails.
        let err = handle_request::<SlaveKind, _>(
            2,
            Request::ReadHoldingRegisters(0, 2),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    #[tokio::test]
    async fn ut_handle_write_single_register_persists() {
        let mem = seeded_memory(&[]);
        let (log, _) = recording_log();
        handle_request::<SlaveKind, _>(1, Request::WriteSingleRegister(1, 99), &mem, &log, false)
            .await
            .unwrap();
        let resp = handle_request::<SlaveKind, _>(
            1,
            Request::ReadHoldingRegisters(1, 1),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(resp, Response::ReadHoldingRegisters(v) if v == vec![99]));
    }

    #[tokio::test]
    async fn ut_handle_verbose_logs_outcome_quiet_when_off() {
        let mem = seeded_memory(&[1, 2]);

        // verbose = true: a "received" line plus a "successful" line.
        let (log, buf) = recording_log();
        handle_request::<SlaveKind, _>(1, Request::ReadHoldingRegisters(0, 2), &mem, &log, true)
            .await
            .unwrap();
        let verbose = buf.lock().unwrap().clone();
        assert_eq!(verbose.len(), 2);
        assert!(verbose[0].contains("received"));
        assert!(verbose[1].contains("successful"));

        // verbose = false: only the "received" line.
        let (log, buf) = recording_log();
        handle_request::<SlaveKind, _>(1, Request::ReadHoldingRegisters(0, 2), &mem, &log, false)
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
        ty: Type,
        len: usize,
        seed: &[u16],
    ) -> Arc<RwLock<Memory<Key<SlaveKind>>>> {
        let key = Key {
            id: SlaveKind { slave_id: 1, kind },
        };
        let mut mem = Memory::<Key<SlaveKind>>::default();
        mem.add_ranges(key.clone(), &MemKind::ReadWrite(ty), &[Range::new(0, len)]);
        if !seed.is_empty() {
            mem.write(key, &ty, &Range::new(0, seed.len()), seed);
        }
        Arc::new(RwLock::new(mem))
    }

    // ---- WriteMultipleCoils: regression for the hard-coded range length bug ----

    #[tokio::test]
    async fn ut_write_multiple_coils_persists_every_bit() {
        let mem = seeded(RegKind::Coil, Type::Coil, 8, &[]);
        let (log, _) = recording_log();
        let coils = vec![true, false, true, true, false];

        let resp = handle_request::<SlaveKind, _>(
            1,
            Request::WriteMultipleCoils(1, coils.clone().into()),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        // Regression: the write range length must equal values.len(), not 1. Before the fix
        // `Memory::write` rejected any multi-coil write (range.length() != values.len()).
        assert!(matches!(resp, Response::WriteMultipleCoils(1, 5)));

        let read =
            handle_request::<SlaveKind, _>(1, Request::ReadCoils(1, 5), &mem, &log, false)
                .await
                .unwrap();
        assert!(matches!(read, Response::ReadCoils(v) if v == coils));
    }

    #[tokio::test]
    async fn ut_write_multiple_coils_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::Coil, Type::Coil, 8, &[]);
        let (log, _) = recording_log();
        // addr 6 + 5 coils overruns the registered [0, 8) region.
        let err = handle_request::<SlaveKind, _>(
            1,
            Request::WriteMultipleCoils(6, vec![true; 5].into()),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- Coil / discrete-input reads ----

    #[tokio::test]
    async fn ut_read_coils_returns_seeded_bits() {
        let mem = seeded(RegKind::Coil, Type::Coil, 4, &[1, 0, 1, 0]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKind, _>(1, Request::ReadCoils(0, 4), &mem, &log, false)
            .await
            .unwrap();
        assert!(matches!(resp, Response::ReadCoils(v) if v == vec![true, false, true, false]));
    }

    #[tokio::test]
    async fn ut_read_coils_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::Coil, Type::Coil, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKind, _>(1, Request::ReadCoils(10, 2), &mem, &log, false)
            .await
            .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    #[tokio::test]
    async fn ut_read_discrete_inputs_returns_seeded_bits() {
        let mem = seeded(RegKind::DiscreteInput, Type::Coil, 3, &[0, 1, 1]);
        let (log, _) = recording_log();
        let resp =
            handle_request::<SlaveKind, _>(1, Request::ReadDiscreteInputs(0, 3), &mem, &log, false)
                .await
                .unwrap();
        assert!(matches!(resp, Response::ReadDiscreteInputs(v) if v == vec![false, true, true]));
    }

    #[tokio::test]
    async fn ut_read_discrete_inputs_unknown_slave_is_illegal_function() {
        let mem = seeded(RegKind::DiscreteInput, Type::Coil, 3, &[]);
        let (log, _) = recording_log();
        let err =
            handle_request::<SlaveKind, _>(2, Request::ReadDiscreteInputs(0, 3), &mem, &log, false)
                .await
                .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- Register reads ----

    #[tokio::test]
    async fn ut_read_input_registers_returns_seeded_values() {
        let mem = seeded(RegKind::InputRegister, Type::Register, 3, &[7, 8, 9]);
        let (log, _) = recording_log();
        let resp =
            handle_request::<SlaveKind, _>(1, Request::ReadInputRegisters(0, 3), &mem, &log, false)
                .await
                .unwrap();
        assert!(matches!(resp, Response::ReadInputRegisters(v) if v == vec![7, 8, 9]));
    }

    #[tokio::test]
    async fn ut_read_input_registers_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::InputRegister, Type::Register, 3, &[]);
        let (log, _) = recording_log();
        let err =
            handle_request::<SlaveKind, _>(1, Request::ReadInputRegisters(2, 5), &mem, &log, false)
                .await
                .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    #[tokio::test]
    async fn ut_read_holding_registers_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKind, _>(
            1,
            Request::ReadHoldingRegisters(3, 4),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- Single writes ----

    #[tokio::test]
    async fn ut_write_single_coil_persists() {
        let mem = seeded(RegKind::Coil, Type::Coil, 4, &[]);
        let (log, _) = recording_log();
        let resp =
            handle_request::<SlaveKind, _>(1, Request::WriteSingleCoil(2, true), &mem, &log, false)
                .await
                .unwrap();
        assert!(matches!(resp, Response::WriteSingleCoil(2, true)));
        let read =
            handle_request::<SlaveKind, _>(1, Request::ReadCoils(2, 1), &mem, &log, false)
                .await
                .unwrap();
        assert!(matches!(read, Response::ReadCoils(v) if v == vec![true]));
    }

    #[tokio::test]
    async fn ut_write_single_coil_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::Coil, Type::Coil, 4, &[]);
        let (log, _) = recording_log();
        let err =
            handle_request::<SlaveKind, _>(1, Request::WriteSingleCoil(9, true), &mem, &log, false)
                .await
                .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    #[tokio::test]
    async fn ut_write_single_register_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKind, _>(
            1,
            Request::WriteSingleRegister(99, 1),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- WriteMultipleRegisters ----

    #[tokio::test]
    async fn ut_write_multiple_registers_persists_all() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 8, &[]);
        let (log, _) = recording_log();
        let resp = handle_request::<SlaveKind, _>(
            1,
            Request::WriteMultipleRegisters(1, vec![11, 22, 33].into()),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(resp, Response::WriteMultipleRegisters(1, 3)));
        let read = handle_request::<SlaveKind, _>(
            1,
            Request::ReadHoldingRegisters(1, 3),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(read, Response::ReadHoldingRegisters(v) if v == vec![11, 22, 33]));
    }

    #[tokio::test]
    async fn ut_write_multiple_registers_out_of_range_is_illegal_function() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKind, _>(
            1,
            Request::WriteMultipleRegisters(3, vec![1, 2, 3].into()),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- ReadWriteMultipleRegisters (reads and writes the same holding region) ----

    #[tokio::test]
    async fn ut_read_write_multiple_registers_writes_then_returns_read() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 8, &[5, 6, 7, 8]);
        let (log, _) = recording_log();
        // Read [0,2), write [2,4) = [77, 88].
        let resp = handle_request::<SlaveKind, _>(
            1,
            Request::ReadWriteMultipleRegisters(0, 2, 2, vec![77, 88].into()),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(resp, Response::ReadWriteMultipleRegisters(v) if v == vec![5, 6]));
        let read = handle_request::<SlaveKind, _>(
            1,
            Request::ReadHoldingRegisters(2, 2),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap();
        assert!(matches!(read, Response::ReadHoldingRegisters(v) if v == vec![77, 88]));
    }

    #[tokio::test]
    async fn ut_read_write_multiple_registers_out_of_range_is_illegal_data_address() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 4, &[1, 2, 3, 4]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKind, _>(
            1,
            Request::ReadWriteMultipleRegisters(0, 2, 10, vec![1, 2].into()),
            &mem,
            &log,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    // ---- Unsupported function codes ----

    #[tokio::test]
    async fn ut_report_server_id_is_illegal_function() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 4, &[]);
        let (log, _) = recording_log();
        let err = handle_request::<SlaveKind, _>(1, Request::ReportServerId, &mem, &log, false)
            .await
            .unwrap_err();
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    // ---- Verbose logging: every arm's success/failure log branch ----

    #[tokio::test]
    async fn ut_verbose_logs_success_for_every_request() {
        macro_rules! ok {
            ($mem:expr, $req:expr) => {{
                let mem = $mem;
                let (log, buf) = recording_log();
                handle_request::<SlaveKind, _>(1, $req, &mem, &log, true)
                    .await
                    .unwrap();
                assert!(
                    buf.lock().unwrap().iter().any(|l| l.contains("successful")),
                    "missing success log line"
                );
            }};
        }
        ok!(
            seeded(RegKind::Coil, Type::Coil, 8, &[1, 0, 1, 0, 1, 0, 1, 0]),
            Request::ReadCoils(0, 4)
        );
        ok!(
            seeded(RegKind::Coil, Type::Coil, 8, &[]),
            Request::WriteSingleCoil(0, true)
        );
        ok!(
            seeded(RegKind::Coil, Type::Coil, 8, &[]),
            Request::WriteMultipleCoils(0, vec![true, false, true].into())
        );
        ok!(
            seeded(RegKind::DiscreteInput, Type::Coil, 4, &[1, 1, 1, 1]),
            Request::ReadDiscreteInputs(0, 4)
        );
        ok!(
            seeded(RegKind::InputRegister, Type::Register, 4, &[1, 2, 3, 4]),
            Request::ReadInputRegisters(0, 4)
        );
        ok!(
            seeded(RegKind::HoldingRegister, Type::Register, 8, &[]),
            Request::WriteSingleRegister(0, 9)
        );
        ok!(
            seeded(RegKind::HoldingRegister, Type::Register, 8, &[]),
            Request::WriteMultipleRegisters(0, vec![1, 2, 3].into())
        );
        ok!(
            seeded(RegKind::HoldingRegister, Type::Register, 8, &[5, 6, 7, 8]),
            Request::ReadWriteMultipleRegisters(0, 2, 2, vec![7, 8].into())
        );
    }

    #[tokio::test]
    async fn ut_verbose_logs_failure_for_every_request() {
        macro_rules! fail {
            ($mem:expr, $req:expr) => {{
                let mem = $mem;
                let (log, buf) = recording_log();
                let _ = handle_request::<SlaveKind, _>(1, $req, &mem, &log, true).await;
                assert!(
                    buf.lock().unwrap().iter().any(|l| l.contains("failed")),
                    "missing failure log line"
                );
            }};
        }
        fail!(
            seeded(RegKind::Coil, Type::Coil, 4, &[]),
            Request::ReadCoils(10, 2)
        );
        fail!(
            seeded(RegKind::Coil, Type::Coil, 4, &[]),
            Request::WriteSingleCoil(9, true)
        );
        fail!(
            seeded(RegKind::Coil, Type::Coil, 4, &[]),
            Request::WriteMultipleCoils(6, vec![true; 5].into())
        );
        fail!(
            seeded(RegKind::DiscreteInput, Type::Coil, 4, &[]),
            Request::ReadDiscreteInputs(10, 2)
        );
        fail!(
            seeded(RegKind::InputRegister, Type::Register, 4, &[]),
            Request::ReadInputRegisters(10, 2)
        );
        fail!(
            seeded(RegKind::HoldingRegister, Type::Register, 4, &[]),
            Request::ReadHoldingRegisters(10, 2)
        );
        fail!(
            seeded(RegKind::HoldingRegister, Type::Register, 4, &[]),
            Request::WriteSingleRegister(99, 1)
        );
        fail!(
            seeded(RegKind::HoldingRegister, Type::Register, 4, &[]),
            Request::WriteMultipleRegisters(3, vec![1, 2, 3].into())
        );
        // Write address out of range -> writable check fails (verbose failure branch).
        fail!(
            seeded(RegKind::HoldingRegister, Type::Register, 4, &[1, 2, 3, 4]),
            Request::ReadWriteMultipleRegisters(0, 2, 10, vec![1, 2].into())
        );
    }

    #[tokio::test]
    async fn ut_unsupported_function_codes_are_illegal() {
        let mem = seeded(RegKind::HoldingRegister, Type::Register, 4, &[]);
        let (log, _) = recording_log();
        for req in [
            Request::MaskWriteRegister(0, 0, 0),
            Request::ReadDeviceIdentification(tokio_modbus::prelude::ReadCode::Basic, 0),
            Request::Custom(0x65, vec![].into()),
        ] {
            let err = handle_request::<SlaveKind, _>(1, req, &mem, &log, false)
                .await
                .unwrap_err();
            assert_eq!(err, ExceptionCode::IllegalFunction);
        }
    }
}
