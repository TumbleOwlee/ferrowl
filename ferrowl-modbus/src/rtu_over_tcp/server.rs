// Crate
use crate::server_core::Server;
use crate::tcp::Config;
use crate::tcp::tls::build_server_tls_config;
use crate::{Error, Key, KeyParams, LogFn, TcpError};

// Workspace
use ferrowl_store::Memory;

// External
use parking_lot::RwLock as MemLock;
use rust_modbus::{Server as ModbusServer, TcpListener, TlsListener};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus RTU-over-TCP server task answering requests from the
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

/// Bind the configured TCP address and spawn the accept loop; each accepted connection answers
/// from the shared `memory` via a [`Server`] using RTU framing (MB-R-113), verbose logging on
/// (MB-R-067). Plain TCP unless `config.tls` is set (MB-R-115), in which case the listener
/// terminates TLS on each accepted connection.
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
    // One service instance answers every accepted connection, so all of them share the
    // one store (MB-R-070). Every server logs per-request outcomes (verbose = true, MB-R-067).
    let server = ModbusServer::new(Server::new(memory, log.clone(), true));
    match &config.tls {
        None => match TcpListener::bind(addr).await {
            Ok(listener) => Ok(tokio::task::spawn(async move {
                server
                    .serve_framed::<rust_modbus::RtuOverTcp>(listener)
                    .await
                    .map_err(Error::Server)
            })),
            Err(e) => Err(Error::Server(e)),
        },
        Some(tls) => {
            let (tls_config, used_fallback) =
                build_server_tls_config(tls, &config.ip).map_err(Error::Tcp)?;
            if used_fallback {
                log.invoke(
                    "No cert_file/key_file/self_signed configured for this TLS \
                     server; falling back to an ephemeral self-signed certificate."
                        .to_string(),
                )
                .await;
            }
            match TlsListener::bind(addr, tls_config).await {
                Ok(listener) => Ok(tokio::task::spawn(async move {
                    server
                        .serve_tls::<rust_modbus::RtuOverTcp>(listener)
                        .await
                        .map_err(Error::Server)
                })),
                Err(e) => Err(Error::Server(e)),
            }
        }
    }
}
