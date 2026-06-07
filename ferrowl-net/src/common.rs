//! Transport-agnostic internals shared by the TCP and RTU client/server implementations.
//!
//! Both transports produce the same concrete `tokio_modbus::client::Context`; only how the
//! context is *constructed* differs (socket connect vs serial open). Everything after that —
//! the read/run loop and the server request handler — is identical and lives here.

use crate::{Command, Error, Key, KeyParams, ModbusError, Operation, RunConfig, SlaveId};

use ferrowl_mem::{Memory, Range, Type};
use tokio_modbus::Request;
use tokio_modbus::prelude::{ExceptionCode, Response};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::time::sleep;
use tokio_modbus::FunctionCode;
use tokio_modbus::client::Context;
use tokio_modbus::prelude::{Client as ModbusClient, Reader, Slave, SlaveContext, Writer};
use tokio_serial::{DataBits, Parity, SerialPortBuilder, StopBits};

use crate::SerialError;

/// Number of consecutive Modbus exceptions tolerated before the client skips the operation.
pub(crate) const MAX_RETRIES: u32 = 3;

/// Build a `tokio_serial` port builder from the optional serial parameters, validating each.
pub(crate) fn serial_config_from(
    path: &str,
    baud_rate: u32,
    data_bits: Option<u8>,
    stop_bits: Option<u8>,
    parity: Option<&str>,
) -> Result<SerialPortBuilder, SerialError> {
    let mut builder = tokio_serial::new(path, baud_rate);
    if let Some(v) = data_bits {
        builder = builder.data_bits(match v {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            8 => DataBits::Eight,
            _ => {
                return Err(SerialError::Configuration(
                    "Invalid data bits specified".to_string(),
                ));
            }
        });
    }
    if let Some(v) = stop_bits {
        builder = builder.stop_bits(match v {
            1 => StopBits::One,
            2 => StopBits::Two,
            _ => {
                return Err(SerialError::Configuration(
                    "Invalid stop bits specified".to_string(),
                ));
            }
        });
    }
    if let Some(v) = parity {
        let v = v.to_lowercase();
        if v == "odd" {
            builder = builder.parity(Parity::Odd);
        } else if v == "even" {
            builder = builder.parity(Parity::Even);
        } else if v == "none" {
            builder = builder.parity(Parity::None);
        } else {
            return Err(SerialError::Configuration(
                "Invalid parity specified".to_string(),
            ));
        }
    }
    Ok(builder)
}

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
        L: AsyncFn(String) -> () + Send + 'static,
        for<'a> L::CallRefFuture<'a>: Send,
    {
        let result = match op.fn_code {
            FunctionCode::ReadCoils => {
                (log)(format!(
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
                (log)(format!(
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
                (log)(format!(
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
                (log)(format!(
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
        L: AsyncFn(String) -> () + Send + 'static,
        S: AsyncFn(String) -> () + Send + 'static,
        for<'a> L::CallRefFuture<'a>: Send,
        for<'a> S::CallRefFuture<'a>: Send,
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
                            let key = Key { id: T::from_slave_fn(operation.slave_id, fc) };
                            if !guard.write_unchecked(key, &range, &values) {
                                log(format!("{s} Failed because of failing memory update for [{start}, {end})."))
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
                                log(format!("{s} request to read [{start}, {end}) successful. Received values {hex_str}."))
                                    .await;
                            }
                            index = (index + 1) % count;
                            retries = 0;
                        }
                        (s, Err(ModbusError::Timeout(e))) => {
                            let _ = self.context.disconnect().await;
                            log(format!(
                                    "{s} request to read [{start}, {end}) timed out. Disconnecting client. [{e:?}]"
                                )).await;
                            return Err(ModbusError::Timeout(e).into());
                        }
                        (s, Err(ModbusError::Error(e))) => {
                            let _ = self.context.disconnect().await;
                            log(format!(
                                    "{s} request to read [{start}, {end}) failed. Disconnecting client. [{e:?}]"
                                )).await;
                            return Err(ModbusError::Error(e).into());
                        }
                        (s, Err(ModbusError::Exception(e))) => {
                            retries += 1;
                            if retries >= MAX_RETRIES {
                                log(format!(
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
                        log("Client gracefully terminated.".to_string()).await;
                        status("Client disconnected".to_string()).await;
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
                                log(format!(
                                    "WriteSingleCoil request to {addr} with {coil} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log(format!(
                                    "WriteSingleCoil request to {addr} with {coil} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log(format!(
                                    "WriteSingleCoil request to {addr} with {coil} invalid. [{e:?}]"
                                ))
                                .await;
                            }
                            Ok(Ok(Ok(_))) => {
                                log(format!(
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
                                log(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log(format!(
                                    "WriteMultipleCoils request to {addr} with {coils:?} failed. [{e:?}]"
                                )).await;
                            }
                            Ok(_) => {
                                log(format!(
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
                                log(format!(
                                    "WriteSingleRegister request to {addr} with {value} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log(format!(
                                    "WriteSingleRegister request to {addr} with {value} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log(format!(
                                    "WriteSingleRegister request to {addr} with {value} invalid. [{e:?}]"
                                )).await;
                            }
                            Ok(_) => {
                                log(format!(
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
                                log(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} timed out. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Timeout(e).into());
                            }
                            Ok(Err(e)) => {
                                let _ = self.context.disconnect().await;
                                log(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} failed. Disconnecting client. [{e:?}]"
                                )).await;
                                return Err(ModbusError::Error(e).into());
                            }
                            Ok(Ok(Err(e))) => {
                                log(format!(
                                    "WriteMultipleRegister request to {addr} with {values:?} invalid. [{e:?}]"
                                )).await;
                            }
                            Ok(_) => {
                                log(format!(
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
    L: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
    for<'a> L::CallRefFuture<'a>: Send,
{
    match request {
        Request::ReadCoils(addr, cnt) => {
            (log)(format!(
                "ReadCoils request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::ReadCoils) };
            let guard = memory.read().await;
            match guard.read(key, &Type::Coil, &Range::new(addr as usize, cnt as usize)) {
                Some(v) => {
                    if verbose {
                        (log)(format!(
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
                        (log)(format!(
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
            (log)(format!(
                "ReadDiscreteInputs request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::ReadDiscreteInputs) };
            let guard = memory.read().await;
            match guard.read(key, &Type::Coil, &Range::new(addr as usize, cnt as usize)) {
                Some(v) => {
                    if verbose {
                        (log)(format!(
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
                        (log)(format!(
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
            (log)(format!(
                "ReadInputRegisters request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::ReadInputRegisters) };
            let guard = memory.read().await;
            match guard.read(key, &Type::Register, &Range::new(addr as usize, cnt as usize)) {
                Some(v) => {
                    if verbose {
                        (log)(format!(
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
                        (log)(format!(
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
            (log)(format!(
                "ReadHoldingRegisters request received for slave ID {} and range [{}, {}).",
                slave,
                addr,
                addr + cnt
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::ReadHoldingRegisters) };
            let guard = memory.read().await;
            match guard.read(key, &Type::Register, &Range::new(addr as usize, cnt as usize)) {
                Some(v) => {
                    if verbose {
                        (log)(format!(
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
                        (log)(format!(
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
            (log)(format!(
                "WriteMultipleRegisters request received for slave ID {}, range [{}, {}), and values {:?}.",
                slave,
                addr,
                addr as usize + values.len(),
                values
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::WriteMultipleRegisters) };
            let mut guard = memory.write().await;
            match guard.write(
                key,
                &Type::Register,
                &Range::new(addr as usize, values.len()),
                &values,
            ) {
                true => {
                    if verbose {
                        (log)(format!(
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
                        (log)(format!(
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
            (log)(format!(
                "WriteSingleRegister request received for slave ID {}, address {}, and value {}.",
                slave, addr, value
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::WriteSingleRegister) };
            let mut guard = memory.write().await;
            match guard.write(key, &Type::Register, &Range::new(addr as usize, 1), &[value]) {
                true => {
                    if verbose {
                        (log)(format!(
                            "WriteSingleRegister request for slave ID {}, address {}, and value {} successful.",
                            slave, addr, value
                        ))
                        .await;
                    }
                    Ok(Response::WriteSingleRegister(addr, value))
                }
                false => {
                    if verbose {
                        (log)(format!(
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
            (log)(format!(
                "WriteMultipleCoils request received for slave ID {}, range [{}, {}), and values {:?}.",
                slave,
                addr,
                addr as usize + values.len(),
                values
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::WriteMultipleCoils) };
            let mut guard = memory.write().await;
            let values: Vec<u16> = values.iter().map(|v| *v as u16).collect();
            match guard.write(key, &Type::Coil, &Range::new(addr as usize, 1), &values) {
                true => {
                    if verbose {
                        (log)(format!(
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
                        (log)(format!(
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
            (log)(format!(
                "WriteSingleCoil request received for slave ID {}, address {}, and value {}.",
                slave, addr, value
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::WriteSingleCoil) };
            let mut guard = memory.write().await;
            let val = value as u16;
            match guard.write(key, &Type::Coil, &Range::new(addr as usize, 1), &[val]) {
                true => {
                    if verbose {
                        (log)(format!(
                            "WriteSingleCoil request for slave ID {}, address {}, and value {} successful.",
                            slave, addr, value
                        ))
                        .await;
                    }
                    Ok(Response::WriteSingleCoil(addr, value))
                }
                false => {
                    if verbose {
                        (log)(format!(
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
            (log)(format!(
                "ReportServerId request received for slave ID {}. Unsupported function.",
                slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
        Request::MaskWriteRegister(_, _, _) => {
            (log)(format!(
                "MaskWriteRegister request received for slave ID {}. Unsupported function.",
                slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
        Request::ReadWriteMultipleRegisters(read_addr, cnt, write_addr, values) => {
            (log)(format!(
                "ReadWriteMultipleRegisrters request received for slave ID {}, read address {}, count {}, write address {}, and values {:?}.",
                slave, read_addr, cnt, write_addr, values
            ))
            .await;
            let key = Key { id: T::from_slave_fn(slave, FunctionCode::ReadWriteMultipleRegisters) };
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
                    (log)(format!(
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
                        (log)(format!(
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
                    (log)(format!(
                        "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} failed.",
                        slave, read_addr, cnt, write_addr, values
                    ))
                    .await;
                }
                return Err(ExceptionCode::IllegalFunction);
            }
            if verbose {
                (log)(format!(
                    "ReadWriteMultipleRegisrters request for slave ID {}, read address {}, count {}, write address {}, and values {:?} successful.",
                    slave, read_addr, cnt, write_addr, values
                ))
                .await;
            }
            Ok(Response::ReadWriteMultipleRegisters(v))
        }
        Request::ReadDeviceIdentification(_, _) => {
            (log)(format!(
                "ReadDeviceIdentification request received for slave ID {}. Unsupported function.",
                slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
        Request::Custom(func, _) => {
            (log)(format!(
                "Custom function {} request received for slave ID {}. Unsupported function.",
                func, slave,
            ))
            .await;
            Err(ExceptionCode::IllegalFunction)
        }
    }
}
