//! Transport-agnostic Modbus client loop shared by the TCP and RTU clients.
//!
//! Both transports produce the same concrete `tokio_modbus::client::Context`; only how the
//! context is *constructed* differs (socket connect vs serial open). Everything after that —
//! the read/run loop and command execution — is identical and lives here.

use crate::{Command, Error, Key, KeyParams, LogFn, ModbusError, Operation, RunConfig};

use ferrowl_store::Memory;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::time::sleep;
use tokio_modbus::FunctionCode;
use tokio_modbus::client::Context;
use tokio_modbus::prelude::{Client as ModbusClient, Reader, Slave, SlaveContext, Writer};

/// Number of consecutive Modbus exceptions tolerated before the client skips the operation.
pub(crate) const MAX_RETRIES: u32 = 3;

/// Starting (and post-success reset) reconnect backoff.
pub(crate) const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// Reconnect backoff cap; doubles from [`INITIAL_BACKOFF`] up to this.
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Owns a connected Modbus client context and drives the read/command loop. Transport-neutral:
/// the TCP and RTU `Client` types each construct the `Context` then hand it here.
pub(crate) struct ClientCore {
    pub(crate) context: Context,
}

impl ClientCore {
    /// Logs the "about to read" intent line shared by every read function code.
    async fn log_read_intent<L>(
        log: &L,
        name: &str,
        slave_id: tokio_modbus::SlaveId,
        start: usize,
        end: usize,
    ) where
        L: LogFn,
    {
        log.invoke(format!(
            "Perform {name} request for slave ID {slave_id} and range [{start}, {end})."
        ))
        .await;
    }

    /// Converts a coil/discrete-input bit vector to the `u16` shape the shared memory store uses.
    fn bits_to_words(bits: Vec<bool>) -> Vec<u16> {
        bits.into_iter().map(|b| if b { 1 } else { 0 }).collect()
    }

    /// Classifies a completed timeout+request result into the single `ModbusError` shape shared
    /// by every read and write outcome.
    fn classify<V>(
        result: Result<
            Result<Result<V, tokio_modbus::ExceptionCode>, tokio_modbus::Error>,
            tokio::time::error::Elapsed,
        >,
    ) -> Result<V, ModbusError> {
        match result {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(ModbusError::Exception(e)),
            Ok(Err(e)) => Err(ModbusError::Error(e)),
            Err(e) => Err(ModbusError::Timeout(e)),
        }
    }

    async fn read<L>(
        &mut self,
        op: &Operation,
        timeout_ms: usize,
        log: &L,
    ) -> (&'static str, Result<Vec<u16>, ModbusError>)
    where
        L: LogFn,
    {
        let start = op.range.start;
        let end = op.range.end;
        let count = (end - start) as u16;
        match op.fn_code {
            FunctionCode::ReadCoils => {
                Self::log_read_intent(log, "ReadCoils", op.slave_id, start, end).await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_coils(start as u16, count),
                )
                .await;
                ("ReadCoils", Self::classify(res).map(Self::bits_to_words))
            }
            FunctionCode::ReadDiscreteInputs => {
                Self::log_read_intent(log, "ReadDiscreteInputs", op.slave_id, start, end).await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_discrete_inputs(start as u16, count),
                )
                .await;
                (
                    "ReadDiscreteInputs",
                    Self::classify(res).map(Self::bits_to_words),
                )
            }
            FunctionCode::ReadInputRegisters => {
                Self::log_read_intent(log, "ReadInputRegisters", op.slave_id, start, end).await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_input_registers(start as u16, count),
                )
                .await;
                ("ReadInputRegisters", Self::classify(res))
            }
            FunctionCode::ReadHoldingRegisters => {
                Self::log_read_intent(log, "ReadHoldingRegisters", op.slave_id, start, end).await;
                self.context.set_slave(Slave(op.slave_id));
                let res = tokio::time::timeout(
                    Duration::from_millis(timeout_ms as u64),
                    self.context.read_holding_registers(start as u16, count),
                )
                .await;
                ("ReadHoldingRegisters", Self::classify(res))
            }
            _ => (
                "Unknown",
                Self::classify(Ok(Ok(Err(tokio_modbus::ExceptionCode::IllegalFunction)))),
            ),
        }
    }

    /// Classifies a completed write result and logs the outcome with the same four-way shape
    /// (timeout / io error / exception / success) shared by every write command. Disconnects and
    /// returns an error on timeout or io error; logs and continues (`Ok(())`) otherwise.
    /// `invalid_word` covers the one wording inconsistency between commands ("invalid" vs.
    /// "failed" for the exception case).
    async fn handle_write_result<V, L>(
        &mut self,
        result: Result<
            Result<Result<V, tokio_modbus::ExceptionCode>, tokio_modbus::Error>,
            tokio::time::error::Elapsed,
        >,
        label: &str,
        detail: &str,
        invalid_word: &str,
        log: &L,
    ) -> Result<(), Error>
    where
        L: LogFn,
    {
        match Self::classify(result) {
            Ok(_) => {
                log.invoke(format!(
                    "{label} request to {detail} successfully executed."
                ))
                .await;
                Ok(())
            }
            Err(ModbusError::Exception(e)) => {
                log.invoke(format!(
                    "{label} request to {detail} {invalid_word}. [{e:?}]"
                ))
                .await;
                Ok(())
            }
            Err(ModbusError::Error(e)) => {
                let _ = self.context.disconnect().await;
                log.invoke(format!(
                    "{label} request to {detail} failed. Disconnecting client. [{e:?}]"
                ))
                .await;
                Err(ModbusError::Error(e).into())
            }
            Err(ModbusError::Timeout(e)) => {
                let _ = self.context.disconnect().await;
                log.invoke(format!(
                    "{label} request to {detail} timed out. Disconnecting client. [{e:?}]"
                ))
                .await;
                Err(ModbusError::Timeout(e).into())
            }
        }
    }

    /// Runs one poll cycle: reads the next operation in rotation and writes the result into
    /// `memory`, advancing (or retrying) the round-robin index. Broken out of `run` so the tick
    /// arm of its `select!` stays a single call. Sets `*had_success` on a successful read, so the
    /// caller's reconnect backoff can tell a live-then-dropped connection from a connection that
    /// never got a single read through.
    #[allow(clippy::too_many_arguments)]
    async fn poll_once<T, L>(
        &mut self,
        operations: &Arc<RwLock<Vec<Operation>>>,
        memory: &Arc<RwLock<Memory<Key<T>>>>,
        timeout_ms: usize,
        log: &L,
        index: &mut usize,
        retries: &mut u32,
        had_success: &mut bool,
    ) -> Result<(), Error>
    where
        T: KeyParams,
        L: LogFn,
    {
        let operations = operations.read().await;
        let count = operations.len();
        if *index >= count {
            *index = 0;
        }
        let operation = operations.get(*index).map(|v| (*v).clone());

        if let Some(operation) = operation {
            let fc = operation.fn_code;
            let range = operation.range.clone();
            let start = range.start;
            let end = range.end;
            match self.read(&operation, timeout_ms, log).await {
                (s, Ok(values)) => {
                    *had_success = true;
                    let mut guard = memory.write().await;
                    let key = Key {
                        id: T::from_slave_fn(operation.slave_id, fc),
                    };
                    if !guard.write_unchecked(key, &range, &values) {
                        log.invoke(format!(
                            "{s} Failed because of failing memory update for [{start}, {end})."
                        ))
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
                    *index = (*index + 1) % count;
                    *retries = 0;
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
                    ))
                    .await;
                    return Err(ModbusError::Error(e).into());
                }
                (s, Err(ModbusError::Exception(e))) => {
                    *retries += 1;
                    if *retries >= MAX_RETRIES {
                        log.invoke(format!(
                            "{s} request to read [{start}, {end}) invalid. [{e}]"
                        ))
                        .await;
                        *index = (*index + 1) % count;
                        *retries = 0;
                    }
                }
            }
        }
        Ok(())
    }

    /// Runs the read/command loop against the connected `context` until a graceful
    /// `Command::Terminate` (or the command channel closing) or a transport error. Returns
    /// whether at least one read succeeded during this run alongside the outcome, so the
    /// caller's reconnect backoff can reset after a connection that was live for a while rather
    /// than one that never got a read through.
    pub(crate) async fn run<T, L, S>(
        mut self,
        operations: Arc<RwLock<Vec<Operation>>>,
        memory: Arc<RwLock<Memory<Key<T>>>>,
        receiver: &mut Receiver<Command>,
        config: RunConfig<L, S>,
    ) -> (bool, Result<(), Error>)
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

        // Wait timeout until first operation
        sleep(Duration::from_millis(delay_ms as u64)).await;

        // `interval_ms` of 0 means "as fast as possible"; tokio's interval requires a non-zero
        // period, and firing every 1ms is indistinguishable from that in practice.
        let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms.max(1) as u64));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut index = 0;
        let mut retries = 0;
        let mut had_success = false;
        loop {
            tokio::select! {
                // Perform next read of registers
                _ = ticker.tick() => {
                    if let Err(e) = self
                        .poll_once(&operations, &memory, timeout_ms, &log, &mut index, &mut retries, &mut had_success)
                        .await
                    {
                        return (had_success, Err(e));
                    }
                }
                // Execute next command if available. `None` means every sender was dropped
                // (e.g. the owning instance was torn down without sending `Terminate`); treat
                // that the same as an explicit `Terminate`.
                cmd = receiver.recv() => match cmd.unwrap_or(Command::Terminate) {
                    Command::Terminate => {
                        let _ = self.context.disconnect().await;
                        log.invoke("Client gracefully terminated.".to_string())
                            .await;
                        status.invoke("Client disconnected".to_string()).await;
                        return (had_success, Ok(()));
                    }
                    Command::WriteSingleCoil(slave, addr, coil) => {
                        self.context.set_slave(Slave(slave));
                        let result = tokio::time::timeout(
                            Duration::from_millis(timeout_ms as u64),
                            self.context.write_single_coil(addr, coil),
                        )
                        .await;
                        if let Err(e) = self
                            .handle_write_result(result, "WriteSingleCoil", &format!("{addr} with {coil}"), "invalid", &log)
                            .await
                        {
                            return (had_success, Err(e));
                        }
                    }
                    Command::WriteMultipleCoils(slave, addr, coils) => {
                        self.context.set_slave(Slave(slave));
                        let result = tokio::time::timeout(
                            Duration::from_millis(timeout_ms as u64),
                            self.context.write_multiple_coils(addr, &coils),
                        )
                        .await;
                        if let Err(e) = self
                            .handle_write_result(result, "WriteMultipleCoils", &format!("{addr} with {coils:?}"), "failed", &log)
                            .await
                        {
                            return (had_success, Err(e));
                        }
                    }
                    Command::WriteSingleRegister(slave, addr, value) => {
                        self.context.set_slave(Slave(slave));
                        let result = tokio::time::timeout(
                            Duration::from_millis(timeout_ms as u64),
                            self.context.write_single_register(addr, value),
                        )
                        .await;
                        if let Err(e) = self
                            .handle_write_result(result, "WriteSingleRegister", &format!("{addr} with {value}"), "invalid", &log)
                            .await
                        {
                            return (had_success, Err(e));
                        }
                    }
                    Command::WriteMultipleRegister(slave, addr, values) => {
                        self.context.set_slave(Slave(slave));
                        let result = tokio::time::timeout(
                            Duration::from_millis(timeout_ms as u64),
                            self.context.write_multiple_registers(addr, &values),
                        )
                        .await;
                        if let Err(e) = self
                            .handle_write_result(result, "WriteMultipleRegister", &format!("{addr} with {values:?}"), "invalid", &log)
                            .await
                        {
                            return (had_success, Err(e));
                        }
                    }
                }
            }
        }
    }

    /// Waits out a reconnect backoff, aborting early on `Command::Terminate` or the command
    /// channel closing (returns `true`). Any other command received while disconnected is
    /// dropped with a log line rather than queued for after reconnect.
    pub(crate) async fn wait_reconnect_backoff<L>(
        receiver: &mut Receiver<Command>,
        backoff: Duration,
        log: &L,
    ) -> bool
    where
        L: LogFn,
    {
        let deadline = tokio::time::Instant::now() + backoff;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return false,
                cmd = receiver.recv() => match cmd {
                    None | Some(Command::Terminate) => return true,
                    Some(_) => {
                        log.invoke(
                            "Command dropped: client is disconnected and reconnecting.".to_string(),
                        )
                        .await;
                    }
                },
            }
        }
    }
}
