// Crate
use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::server_core::Server;
use crate::{Error, Key, KeyParams, LogFn, SerialError};

// Workspace
use ferrowl_store::Memory;

// External
use parking_lot::RwLock as MemLock;
use rust_modbus::{Rtu, Server as ModbusServer, open_serial};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus RTU server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    /// Opens the configured serial port and spawns the serve loop as a
    /// tokio task. `log` receives log lines.
    pub async fn spawn<L>(&self, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
    {
        let guard = self.config.read().await;
        run(&guard, self.memory.clone(), log).await
    }
}

/// Open the configured serial port and spawn the RTU serve loop, answering from the shared `memory`
/// via a [`Server`] (verbose logging off).
async fn run<T, L>(
    config: &Config,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    log: L,
) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    let serial = serial_config_from(
        config.baud_rate,
        config.data_bits,
        config.stop_bits,
        config.parity.as_deref(),
    )?;
    match open_serial::<Rtu>(&config.path, serial) {
        Ok(transport) => {
            // RTU servers stay quiet on per-request outcomes (verbose = false).
            let server = ModbusServer::new(Server::new(memory, log, false));
            // One port, one link, no accept loop (MB-R-074). The default `ServerConfig` filters
            // by no unit id, so every slave id with declared regions is served (MB-R-065).
            Ok(tokio::task::spawn(async move {
                server.serve_link(transport).await.map_err(Error::Server)
            }))
        }
        Err(e) => Err(SerialError::Error(e).into()),
    }
}
