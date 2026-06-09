use crate::client_core::ClientCore;
use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::{Command, Error, Key, KeyParams, LogFn, Operation, RunConfig, SerialError};

use ferrowl_mem::Memory;
use tokio::task::JoinHandle;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio_modbus::prelude::{Slave, rtu};
use tokio_serial::SerialStream;

/// Builds and spawns a Modbus RTU client task that polls `operations` into
/// the shared `memory` and executes incoming [`Command`]s.
pub struct ClientBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: Arc<RwLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ClientBuilder<T> {
    pub fn new(
        config: Arc<RwLock<Config>>,
        operations: Arc<RwLock<Vec<Operation>>>,
        memory: Arc<RwLock<Memory<Key<T>>>>,
    ) -> Self {
        Self {
            config,
            operations,
            memory,
        }
    }

    /// Opens the serial port and spawns the client loop as a tokio task.
    /// `log` receives log lines, `status` receives connection status
    /// updates, and `receiver` delivers write/terminate [`Command`]s.
    pub async fn spawn<L, S>(
        &self,
        receiver: Receiver<Command>,
        log: L,
        status: S,
    ) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn,
        S: LogFn,
    {
        let guard = self.config.read().await;
        let client = Client::connect(&guard).await?;
        let operations = self.operations.clone();
        let memory = self.memory.clone();
        let config = RunConfig {
            log,
            status,
            timeout_ms: guard.timeout_ms,
            delay_ms: guard.delay_ms,
            interval_ms: guard.interval_ms,
        };
        Ok(tokio::task::spawn(async move {
            client
                .core
                .run::<T, _, _>(operations, memory, receiver, config)
                .await
        }))
    }
}

/// A connected Modbus RTU client. Connection setup is serial-specific; the read/command loop is
/// shared via the internal `ClientCore`.
pub struct Client {
    pub(crate) core: ClientCore,
}

impl Client {
    /// Opens the configured serial port and attaches it to the configured
    /// slave address.
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let builder = serial_config_from(
            &config.path,
            config.baud_rate,
            config.data_bits,
            config.stop_bits,
            config.parity.as_deref(),
        )?;
        match SerialStream::open(&builder).map(|s| rtu::attach_slave(s, Slave(config.slave))) {
            Ok(context) => Ok(Self {
                core: ClientCore { context },
            }),
            Err(e) => Err(SerialError::Error(e).into()),
        }
    }
}
