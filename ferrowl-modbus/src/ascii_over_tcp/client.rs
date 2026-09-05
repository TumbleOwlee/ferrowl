use crate::client_core::{ClientCore, connect_tcp_family, spawn_tcp_family_client};
use crate::tcp::Config;
use crate::tcp::tls::{ClientStream, SelfSignedCache};
use crate::{Command, ConnectedCell, Error, Key, KeyParams, LogFn, Operation};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use rust_modbus::{Ascii, FrameTransport};
use tokio::task::JoinHandle;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

/// Builds and spawns a Modbus ASCII-over-TCP client task that polls `operations` into
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
        Ok(spawn_tcp_family_client::<T, Ascii, L, S>(
            self.config.clone(),
            self.operations.clone(),
            self.memory.clone(),
            self.cache.clone(),
            receiver,
            log,
            status,
        ))
    }
}

/// A connected Modbus ASCII-over-TCP client. Connection setup mirrors plain TCP/RtuOverTcp
/// (MB-R-125/126); the read/command loop is shared via the internal `ClientCore`, over a socket
/// carrying ASCII framing.
pub struct Client {
    pub(crate) core: ClientCore<FrameTransport<ClientStream, Ascii>, Ascii>,
}

impl Client {
    /// Opens a TCP connection to `config.ip:config.port`, bounded by the
    /// configured timeout. Plain TCP unless `config.tls` is set (MB-R-127), in which
    /// case the same timeout bounds the TCP connect and the TLS handshake together.
    pub async fn connect(config: &Config, cache: &SelfSignedCache) -> Result<Self, Error> {
        connect_tcp_family::<Ascii>(config, cache)
            .await
            .map(|core| Self { core })
    }
}
