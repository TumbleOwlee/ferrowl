// Crate
use crate::server_core::Server;
use crate::tcp::Config;
use crate::{Error, Key, KeyParams, LogFn, TcpError};

// Workspace
use ferrowl_store::Memory;

// External
use parking_lot::RwLock as MemLock;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_modbus::server::tcp::{Server as TcpServer, accept_tcp_connection};

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
            let server = TcpServer::new(listener);
            let memory = memory.clone();
            let log = log.clone();
            Ok(tokio::task::spawn(async move {
                // TCP servers log per-request outcomes (verbose = true).
                let new_request_handler =
                    |_socket_addr| Ok(Some(Server::new(memory.clone(), log.clone(), true)));
                let on_connected = |stream, socket_addr| async move {
                    accept_tcp_connection(stream, socket_addr, new_request_handler)
                };
                let on_process_log = log.clone();
                // `on_process_error` is a sync callback (not awaited by `tokio_modbus`), so the
                // log line is emitted on a detached task instead of blocking a worker thread to
                // await it inline.
                let on_process_error = move |err| {
                    let on_process_log = on_process_log.clone();
                    tokio::task::spawn(async move {
                        on_process_log
                            .invoke(format!("Server processing failed. [{}]", err))
                            .await;
                    });
                };
                server
                    .serve(&on_connected, on_process_error)
                    .await
                    .map_err(Error::Server)
            }))
        }
        Err(e) => Err(Error::Server(e)),
    }
}
