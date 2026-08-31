use crate::client_core::{BoxedConnect, ClientCore, spawn_client_task};
use crate::udp::Config;
use crate::{Command, ConnectedCell, Error, Key, KeyParams, LogFn, Operation, TcpError};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use rust_modbus::{Client as ModbusClient, UdpConfig, UdpTransport, connect_udp};
use tokio::task::JoinHandle;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

/// Builds and spawns a Modbus UDP client task that polls `operations` into
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

    /// Associates with the configured peer and spawns the client loop as a tokio task. `log`
    /// receives log lines, `status` receives connection status updates, and `receiver` delivers
    /// write/terminate [`Command`]s.
    ///
    /// With `config.reconnect` set (the default), a lost association does not end the task: it
    /// logs, waits an exponential backoff (capped, reset after a run that got at least one read
    /// through), and retries. `Command::Terminate` (or the channel closing) aborts a backoff
    /// wait immediately. With `config.reconnect` unset, a transport error ends the task exactly
    /// as before this behavior was added.
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
        Ok(spawn_client_task(
            self.config.clone(),
            self.operations.clone(),
            self.memory.clone(),
            receiver,
            log,
            status,
            move |cfg: &Config| -> BoxedConnect<'_, UdpTransport<rust_modbus::Tcp>, rust_modbus::Tcp> {
                Box::pin(async move { Client::connect(cfg).await.map(|client| client.core) })
            },
        ))
    }
}

/// A connected Modbus UDP client. Associating with the peer is local-only (no handshake,
/// MB-R-117); the read/command loop is shared via the internal `ClientCore`, over
/// `UdpTransport<Tcp>` (MBAP framing) directly — no `FrameTransport` wrapping (unlike every
/// other transport): `UdpTransport` already implements `rust_modbus::ClientTransport` itself.
pub struct Client {
    pub(crate) core: ClientCore<UdpTransport<rust_modbus::Tcp>, rust_modbus::Tcp>,
}

impl Client {
    /// Binds an ephemeral local socket and associates it with `config.ip:config.port`
    /// (MB-R-117). Unlike `tcp::Client::connect`, this is **not** wrapped in
    /// `tokio::time::timeout(config.timeout_ms, ...)`: the bind/associate step performs no I/O
    /// to time out — `timeout_ms` bounds each individual request instead, already enforced
    /// generically by `ClientCore::read`/`handle_write_result` (MB-R-040).
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
            .parse()
            .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
        let transport: UdpTransport<rust_modbus::Tcp> = connect_udp(addr, UdpConfig::default())
            .await
            .map_err(|e| Error::from(TcpError::Error(e)))?;
        Ok(Self {
            core: ClientCore {
                client: ModbusClient::new(transport),
            },
        })
    }
}
