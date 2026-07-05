use crate::client_core::{ClientCore, INITIAL_BACKOFF, MAX_BACKOFF};
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

    /// Connects to the configured endpoint and spawns the client loop as a tokio task. `log`
    /// receives log lines, `status` receives connection status updates, and `receiver` delivers
    /// write/terminate [`Command`]s.
    ///
    /// With `config.reconnect` set (the default), a lost or refused connection does not end the
    /// task: it logs, waits an exponential backoff (capped, reset after a run that got at least
    /// one read through), and reconnects. `Command::Terminate` (or the channel closing) aborts a
    /// backoff wait immediately. With `config.reconnect` unset, a transport error ends the task
    /// exactly as before this behavior was added.
    pub async fn spawn<L, S>(
        &self,
        mut receiver: Receiver<Command>,
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
            let mut backoff = INITIAL_BACKOFF;
            loop {
                let guard = config.read().await;
                let reconnect = guard.reconnect;
                let run_config = RunConfig {
                    log: log.clone(),
                    status: status.clone(),
                    timeout_ms: guard.timeout_ms,
                    delay_ms: guard.delay_ms,
                    interval_ms: guard.interval_ms,
                };
                let connected = Client::connect(&guard).await;
                drop(guard);

                let client = match connected {
                    Ok(client) => client,
                    Err(e) => {
                        if !reconnect {
                            log.invoke(format!("{e} Reconnect disabled; client stopping."))
                                .await;
                            status.invoke("Client disconnected".to_string()).await;
                            return Err(e);
                        }
                        log.invoke(format!("{e} Reconnecting in {}s.", backoff.as_secs()))
                            .await;
                        if ClientCore::wait_reconnect_backoff(&mut receiver, backoff, &log).await {
                            status.invoke("Client disconnected".to_string()).await;
                            return Ok(());
                        }
                        backoff = (backoff * 2).min(MAX_BACKOFF);
                        continue;
                    }
                };

                let (had_success, result) = client
                    .core
                    .run::<T, _, _>(
                        operations.clone(),
                        memory.clone(),
                        &mut receiver,
                        run_config,
                    )
                    .await;
                match result {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        if !reconnect {
                            // run() already logged the underlying disconnect; just surface the
                            // status change before the task ends.
                            status.invoke("Client disconnected".to_string()).await;
                            return Err(e);
                        }
                        if had_success {
                            backoff = INITIAL_BACKOFF;
                        }
                        log.invoke(format!("{e} Reconnecting in {}s.", backoff.as_secs()))
                            .await;
                        if ClientCore::wait_reconnect_backoff(&mut receiver, backoff, &log).await {
                            status.invoke("Client disconnected".to_string()).await;
                            return Ok(());
                        }
                        backoff = (backoff * 2).min(MAX_BACKOFF);
                    }
                }
            }
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
