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
            match guard.write(key, &Type::Coil, &Range::new(addr as usize, 1), &values) {
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
}
