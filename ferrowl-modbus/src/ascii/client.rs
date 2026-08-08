use crate::client_core::{ClientCore, ConnectAttempt};
use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::{Command, Error, Key, KeyParams, LogFn, Operation, SerialError};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use tokio::task::JoinHandle;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

use rust_modbus::{Ascii, Client as ModbusClient, FrameTransport, SerialStream, open_serial};

/// Builds and spawns a Modbus ASCII client task that polls `operations` into
/// the shared `memory` and executes incoming [`Command`]s.
pub struct ClientBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ClientBuilder<T> {
    pub fn new(
        config: Arc<RwLock<Config>>,
        operations: Arc<RwLock<Vec<Operation>>>,
        memory: Arc<MemLock<Memory<Key<T>>>>,
    ) -> Self {
        Self {
            config,
            operations,
            memory,
        }
    }

    /// Opens the serial port and spawns the client loop as a tokio task. `log` receives log
    /// lines, `status` receives connection status updates, and `receiver` delivers
    /// write/terminate [`Command`]s.
    ///
    /// With `config.reconnect` set (the default), a lost or unopenable port does not end the
    /// task: it logs, waits an exponential backoff (capped, reset after a run that got at least
    /// one read through), and retries. `Command::Terminate` (or the channel closing) aborts a
    /// backoff wait immediately. With `config.reconnect` unset, a transport error ends the task
    /// exactly as before this behavior was added.
    pub async fn spawn<L, S>(
        &self,
        receiver: Receiver<Command>,
        log: L,
        status: S,
    ) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
        S: LogFn + Clone,
    {
        let config = self.config.clone();
        let operations = self.operations.clone();
        let memory = self.memory.clone();
        Ok(tokio::task::spawn(async move {
            ClientCore::run_reconnect_loop(receiver, log, status, operations, memory, move || {
                let config = config.clone();
                async move {
                    let guard = config.read().await;
                    let attempt = ConnectAttempt {
                        reconnect: guard.reconnect,
                        timeout_ms: guard.timeout_ms,
                        delay_ms: guard.delay_ms,
                        interval_ms: guard.interval_ms,
                        client: Client::connect(&guard).await.map(|client| client.core),
                    };
                    drop(guard);
                    attempt
                }
            })
            .await
        }))
    }
}

/// A connected Modbus ASCII client. Connection setup is serial-specific; the read/command loop
/// is shared via the internal `ClientCore`, over a serial port carrying ASCII framing.
pub struct Client {
    pub(crate) core: ClientCore<FrameTransport<SerialStream, Ascii>, Ascii>,
}

impl Client {
    /// Opens the configured serial port under ASCII framing (MB-R-122).
    ///
    /// The port is not bound to a slave address: each request carries the slave id of the
    /// operation or command that issued it (MB-R-048).
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let serial = serial_config_from(
            config.baud_rate,
            config.data_bits,
            config.stop_bits,
            config.parity.as_deref(),
        )?;
        match open_serial::<Ascii>(&config.path, serial) {
            Ok(transport) => Ok(Self {
                core: ClientCore {
                    client: ModbusClient::new(transport),
                },
            }),
            Err(e) => Err(SerialError::Error(e).into()),
        }
    }
}
