//! Transport-agnostic Modbus client loop shared by the TCP and RTU clients.
//!
//! Both transports produce the same concrete `tokio_modbus::client::Context`; only how the
//! context is *constructed* differs (socket connect vs serial open). Everything after that —
//! the read/run loop and command execution — is identical and lives here.

use crate::{Command, Error, Key, KeyParams, LogFn, ModbusError, Operation, RunConfig};

use ferrowl_mem::Memory;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::time::sleep;
use tokio_modbus::FunctionCode;
use tokio_modbus::client::Context;
use tokio_modbus::prelude::{Client as ModbusClient, Reader, Slave, SlaveContext, Writer};

/// Number of consecutive Modbus exceptions tolerated before the client skips the operation.
pub(crate) const MAX_RETRIES: u32 = 3;

/// Owns a connected Modbus client context and drives the read/command loop. Transport-neutral:
/// the TCP and RTU `Client` types each construct the `Context` then hand it here.
pub(crate) struct ClientCore {
    pub(crate) context: Context,
}

impl ClientCore {
    async fn read<L>(
        &mut self,
        op: &Operation,
        timeout_ms: usize,
        log: &L,
    ) -> (&'static str, Result<Vec<u16>, ModbusError>)
    where
        L: LogFn,
    {
        let result = match op.fn_code {
            FunctionCode::ReadCoils => {
                log.invoke(format!(
                    "Perform ReadCoils request for slave ID {} and range [{}, {}).",
                    op.slave_id, op.range.start, op.range.end,
                ))
                .await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_coils(
                        op.range.start as u16,
                        (op.range.end - op.range.start) as u16,
                    ),
                )
                .await
                .map(|r| {
                    r.map(|v| v.map(|b| b.into_iter().map(|e| if e { 1 } else { 0 }).collect()))
                });
                ("ReadCoils", res)
            }
            FunctionCode::ReadDiscreteInputs => {
                log.invoke(format!(
                    "Perform ReadDiscreteInputs request for slave ID {} and range [{}, {}).",
                    op.slave_id, op.range.start, op.range.end,
                ))
                .await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_discrete_inputs(
                        op.range.start as u16,
                        (op.range.end - op.range.start) as u16,
                    ),
                )
                .await
                .map(|r| {
                    r.map(|v| v.map(|b| b.into_iter().map(|e| if e { 1 } else { 0 }).collect()))
                });
                ("ReadDiscreteInputs", res)
            }
            FunctionCode::ReadInputRegisters => {
                log.invoke(format!(
                    "Perform ReadInputRegisters request for slave ID {} and range [{}, {}).",
                    op.slave_id, op.range.start, op.range.end,
                ))
                .await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_input_registers(
                        op.range.start as u16,
                        (op.range.end - op.range.start) as u16,
                    ),
                )
                .await;
                ("ReadInputRegisters", res)
            }
            FunctionCode::ReadHoldingRegisters => {
                log.invoke(format!(
                    "Perform ReadHoldingRegisters request for slave ID {} and range [{}, {}).",
                    op.slave_id, op.range.start, op.range.end,
                ))
                .await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_holding_registers(
                        op.range.start as u16,
                        (op.range.end - op.range.start) as u16,
                    ),
                )
                .await;
                ("ReadHoldingRegisters", res)
            }
            _ => (
                "Unknown",
                Ok(Ok(Err(tokio_modbus::ExceptionCode::IllegalFunction))),
            ),
        };
        match result {
            (s, Ok(Ok(Ok(v)))) => (s, Ok(v)),
            (s, Ok(Ok(Err(e)))) => (s, Err(ModbusError::Exception(e))),
            (s, Ok(Err(e))) => (s, Err(ModbusError::Error(e))),
            (s, Err(e)) => (s, Err(ModbusError::Timeout(e))),
        }
    }

    pub(crate) fn interval_elapsed(&self, since: &mut Option<Instant>, interval_ms: usize) -> bool {
        let now = Instant::now();
        match since {
            Some(time) => {
                let duration = now.duration_since(*time);
                if duration.as_millis() > interval_ms as u128 {
                    *since = Some(now);
                    true
                } else {
                    false
                }
            }
            None => {
                *since = Some(now);
                true
            }
        }
    }

    pub(crate) async fn run<T, L, S>(
        mut self,
        operations: Arc<RwLock<Vec<Operation>>>,
        memory: Arc<RwLock<Memory<Key<T>>>>,
        mut receiver: Receiver<Command>,
        config: RunConfig<L, S>,
    ) -> Result<(), Error>
    where
        T: KeyParams,
        L: LogFn,
        S: LogFn,
    {
        let RunConfig {
            log,
            status,
            timeout_ms,
            delay_ms,
            interval_ms,
        } = config;
        let mut time: Option<Instant> = None;

        // Wait timeout until first operation
        sleep(Duration::from_millis(delay_ms as u64)).await;

        let mut index = 0;
        let mut retries = 0;
        loop {
            // Perform next read of registers
            if self.interval_elapsed(&mut time, interval_ms) {
                let operations = operations.read().await;
                let count = operations.len();
                if index >= count {
                    index = 0;
                }
                let operation = operations.get(index).map(|v| (*v).clone());

                if let Some(operation) = operation {
                    let fc = operation.fn_code;
                    let range = operation.range.clone();
                    let start = range.start;
                    let end = range.end;
                    match self.read(&operation, timeout_ms, &log).await {
                        (s, Ok(values)) => {
                            let mut guard = memory.write().await;
                            let key = Key {
                                id: T::from_slave_fn(operation.slave_id, fc),
                            };
                            if !guard.write_unchecked(key, &range, &values) {
                                log.invoke(format!("{s} Failed because of failing memory update for [{start}, {end})."))
                                    .await;
                            } else {
                                let mut hex_str = String::with_capacity(values.len() * 3 + 4);
                                hex_str += "[";
                                let mut first = true;
                                for v in values.iter() {
                                    if !first {
                                        hex_str += &format!(" {:04x}", *v);
                                    } else {
                                        hex_str += &format!("{:04x}", *v);
                                    }
                                    first = false;
                                }
                                hex_str += "]";
                                log.invoke(format!("{s} request to read [{start}, {end}) successful. Received values {hex_str}."))
                                    .await;
                            }
                            index = (index + 1) % count;
                            retries = 0;
                        }
                        (s, Err(ModbusError::Timeout(e))) => {
                            let _ = self.context.disconnect().await;
                            log.invoke(format!(
                                    "{s} request to read [{start}, {end}) timed out. Disconnecting client. [{e:?}]"
                                )).await;
                            return Err(ModbusError::Timeout(e).into());
                        }
                        (s, Err(ModbusError::Error(e))) => {
                            let _ = self.context.disconnect().await;
                            log.invoke(format!(
                                    "{s} request to read [{start}, {end}) failed. Disconnecting client. [{e:?}]"
                                )).await;
                            return Err(ModbusError::Error(e).into());
                        }
                        (s, Err(ModbusError::Exception(e))) => {
                            retries += 1;
                            if retries >= MAX_RETRIES {
                                log.invoke(format!(
                                    "{s} request to read [{start}, {end}) invalid. [{e}]"
                                ))
                                .await;
                                index = (index + 1) % count;
                                retries = 0;
                            }
                        }
                    }
                }
            }

            // Execute next command if available
            if let Ok(cmd) = receiver.try_recv() {
                match cmd {
                    Command::Terminate => {
                        let _ = self.context.disconnect().await;
                        log.invoke("Client gracefully terminated.".to_string())
                            .await;
                        status.invoke("Client disconnected".to_string()).await;
                        return Ok(());
                    }
                    Command::WriteSingleCoil(slave, addr, coil) => {
                        self.context.set_slave(Slave(slave));
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout_ms as u64),
                            self.context.write_single_coil(addr, coil),
                        )
                        .await
                        {
                            Err(e) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteSingleCoil request to {addr} with {coil} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteSingleCoil request to {addr} with {coil} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log.invoke(format!(
                                    "WriteSingleCoil request to {addr} with {coil} invalid. [{e:?}]"
                                ))
                                .await;
                            }
                            Ok(Ok(Ok(_))) => {
                                log.invoke(format!(
                                    "WriteSingleCoil request to {addr} with {coil} successfully executed."
                                )).await;
                            }
                        }
                    }
                    Command::WriteMultipleCoils(slave, addr, coils) => {
                        self.context.set_slave(Slave(slave));
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout_ms as u64),
                            self.context.write_multiple_coils(addr, &coils),
                        )
                        .await
                        {
                            Err(e) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log.invoke(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} failed. [{e:?}]"
                                )).await;
                            }
                            Ok(_) => {
                                log.invoke(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} successfully executed."
                                )).await;
                            }
                        }
                    }
                    Command::WriteSingleRegister(slave, addr, value) => {
                        self.context.set_slave(Slave(slave));
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout_ms as u64),
                            self.context.write_single_register(addr, value),
                        )
                        .await
                        {
                            Err(e) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteSingleRegister request to {addr} with {value} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteSingleRegister request to {addr} with {value} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log.invoke(format!(
                                    "WriteSingleRegister request to {addr} with {value} invalid. [{e:?}]"
                                )).await;
                            }
                            Ok(_) => {
                                log.invoke(format!(
                                    "WriteSingleRegister request to {addr} with {value} successfully executed."
                                )).await;
                            }
                        }
                    }
                    Command::WriteMultipleRegister(slave, addr, values) => {
                        self.context.set_slave(Slave(slave));
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout_ms as u64),
                            self.context.write_multiple_registers(addr, &values),
                        )
                        .await
                        {
                            Err(e) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log.invoke(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log.invoke(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} invalid. [{e:?}]"
                                )).await;
                            }
                            Ok(_) => {
                                log.invoke(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} successfully executed."
                                )).await;
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    }
}
