use crate::client_core::{ClientCore, ConnectAttempt};
use crate::tcp::Config;
use crate::tcp::tls::{ClientStream, build_client_tls_config};
use crate::{Command, Error, Key, KeyParams, LogFn, Operation, TcpError};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use rust_modbus::{Client as ModbusClient, FrameTransport, TcpConfig, connect_tcp, connect_tls};
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

/// A connected Modbus TCP client. Connection setup is TCP-specific; the read/command loop is
/// shared via the internal `ClientCore`, over a socket carrying Modbus TCP framing.
pub struct Client {
    pub(crate) core: ClientCore<ClientStream, rust_modbus::Tcp>,
}

impl Client {
    /// Opens a TCP connection to `config.ip:config.port`, bounded by the
    /// configured timeout. Plain TCP unless `config.tls` is set (MB-R-104), in which
    /// case the same timeout bounds the TCP connect and the TLS handshake together.
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
            .parse()
            .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
        let tls_config = config
            .tls
            .as_ref()
            .map(build_client_tls_config)
            .transpose()?;
        let attempt = async {
            match tls_config {
                None => connect_tcp(addr, TcpConfig::default())
                    .await
                    .map(|t| ClientStream::Plain(t.into_inner())),
                Some(tls) => connect_tls(addr, TcpConfig::default(), tls)
                    .await
                    .map(|t| ClientStream::Tls(Box::new(t.into_inner()))),
            }
        };
        match tokio::time::timeout(
            std::time::Duration::from_millis(config.timeout_ms as u64),
            attempt,
        )
        .await
        {
            Ok(Ok(stream)) => Ok(Self {
                core: ClientCore {
                    client: ModbusClient::<_, _>::new(FrameTransport::new(stream)),
                },
            }),
            Ok(Err(e)) => Err(TcpError::Error(e).into()),
            Err(e) => Err(TcpError::Timeout(e).into()),
        }
    }
}
