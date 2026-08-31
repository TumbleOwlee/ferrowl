use crate::client_core::{BoxedConnect, ClientCore, connect_tcp_family, spawn_client_task};
use crate::tcp::Config;
use crate::tcp::tls::{ClientStream, SelfSignedCache};
use crate::{Command, ConnectedCell, Error, Key, KeyParams, LogFn, Operation};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use rust_modbus::FrameTransport;
use tokio::task::JoinHandle;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

/// Builds and spawns a Modbus RTU-over-TCP client task that polls `operations` into
/// the shared `memory` and executes incoming [`Command`]s.
pub struct ClientBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    cache: SelfSignedCache,
}

impl<T: KeyParams> ClientBuilder<T> {
    pub fn new(
        config: Arc<RwLock<Config>>,
        operations: Arc<RwLock<Vec<Operation>>>,
        memory: Arc<MemLock<Memory<Key<T>>>>,
        cache: SelfSignedCache,
    ) -> Self {
        Self {
            config,
            operations,
            memory,
            cache,
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
    /// with that error.
    pub async fn spawn<L, S>(
        &self,
        receiver: Receiver<Command>,
        log: L,
        status: S,
    ) -> Result<(JoinHandle<Result<(), Error>>, ConnectedCell), Error>
    where
        L: LogFn + Clone,
        S: LogFn + Clone,
    {
        let cache = self.cache.clone();
        Ok(spawn_client_task(
            self.config.clone(),
            self.operations.clone(),
            self.memory.clone(),
            receiver,
            log,
            status,
            move |cfg: &Config| -> BoxedConnect<
                '_,
                FrameTransport<ClientStream, rust_modbus::RtuOverTcp>,
                rust_modbus::RtuOverTcp,
            > {
                let cache = cache.clone();
                Box::pin(
                    async move { Client::connect(cfg, &cache).await.map(|client| client.core) },
                )
            },
        ))
    }
}

/// A connected Modbus RTU-over-TCP client. Connection setup mirrors plain TCP (MB-R-113); the
/// read/command loop is shared via the internal `ClientCore`, over a socket carrying RTU framing.
pub struct Client {
    pub(crate) core:
        ClientCore<FrameTransport<ClientStream, rust_modbus::RtuOverTcp>, rust_modbus::RtuOverTcp>,
}

impl Client {
    /// Opens a TCP connection to `config.ip:config.port`, bounded by the
    /// configured timeout. Plain TCP unless `config.tls` is set (MB-R-115), in which
    /// case the same timeout bounds the TCP connect and the TLS handshake together.
    pub async fn connect(config: &Config, cache: &SelfSignedCache) -> Result<Self, Error> {
        connect_tcp_family::<rust_modbus::RtuOverTcp>(config, cache)
            .await
            .map(|core| Self { core })
    }
}
