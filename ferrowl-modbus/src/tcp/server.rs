// Crate
use crate::server_core::Server;
use crate::tcp::Config;
use crate::{Error, Key, KeyParams, LogFn, TcpError};

// Workspace
use ferrowl_store::Memory;

// External
use parking_lot::RwLock as MemLock;
use rust_modbus::{Server as ModbusServer, TcpListener};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus TCP server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    /// Binds the configured listen address and spawns the accept loop as a
    /// tokio task. `log` receives log lines.
    pub async fn spawn<L>(&self, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
    {
        let guard = self.config.read().await;
        run(&guard, self.memory.clone(), log).await
    }
}

/// Bind the configured TCP address and spawn the accept loop; each accepted connection answers from
/// the shared `memory` via a [`Server`] (verbose logging on).
async fn run<T, L>(
    config: &Config,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    log: L,
) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
        .parse()
        .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
    match TcpListener::bind(addr).await {
        Ok(listener) => {
            // One service instance answers every accepted connection, so all of them share the
            // one store (MB-R-070). TCP servers log per-request outcomes (verbose = true).
            let server = ModbusServer::new(Server::new(memory, log, true));
            Ok(tokio::task::spawn(async move {
                server.serve(listener).await.map_err(Error::Server)
            }))
        }
        Err(e) => Err(Error::Server(e)),
    }
}
