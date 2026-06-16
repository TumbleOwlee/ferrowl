use crate::client_core::ClientCore;
use crate::tcp::Config;
use crate::{Command, Error, Key, KeyParams, LogFn, Operation, RunConfig, TcpError};

use ferrowl_store::Memory;
use tokio::task::JoinHandle;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

/// Builds and spawns a Modbus TCP client task that polls `operations` into
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

    /// Connects to the configured endpoint and spawns the client loop as a
    /// tokio task. `log` receives log lines, `status` receives connection
    /// status updates, and `receiver` delivers write/terminate [`Command`]s.
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

/// A connected Modbus TCP client. Connection setup is TCP-specific; the read/command loop is
/// shared via the internal `ClientCore`.
pub struct Client {
    pub(crate) core: ClientCore,
}

impl Client {
    /// Opens a TCP connection to `config.ip:config.port`, bounded by the
    /// configured timeout.
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
            .parse()
            .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
        match tokio::time::timeout(
            std::time::Duration::from_millis(config.timeout_ms as u64),
            tokio_modbus::client::tcp::connect(addr),
        )
        .await
        {
            Ok(Ok(context)) => Ok(Self {
                core: ClientCore { context },
            }),
            Ok(Err(e)) => Err(TcpError::Error(e).into()),
            Err(e) => Err(TcpError::Timeout(e).into()),
        }
    }
}
