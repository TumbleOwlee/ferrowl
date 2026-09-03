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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udp::Config;
    use ferrowl_store::Range;
    use rust_modbus::{FunctionCode, UnitId};
    use std::time::{Duration, Instant};

    /// A `LogFn` that discards every line; only timings and results are asserted here.
    fn silent_log() -> impl LogFn + Clone {
        |_s: String| async move {}
    }

    #[tokio::test]
    /// MB-R-180 — `timeout_ms` bounds each request through `ClientCore::read`, not the
    /// bind/associate step in `Client::connect`, which performs no I/O and cannot time out.
    async fn ut_timeout_ms_bounds_each_request_not_the_associate() {
        let guard = ferrowl_test_support::reserve_udp_port();

        let cfg = Config {
            ip: "127.0.0.1".into(),
            port: guard.port(),
            timeout_ms: 300,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: false,
        };

        let t0 = Instant::now();
        let mut client = Client::connect(&cfg)
            .await
            .expect("associate is local-only");
        assert!(
            t0.elapsed() < Duration::from_millis(200),
            "the bind/associate must not be timed out"
        );

        let log = silent_log();
        let op = Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 2),
        };

        for _ in 0..2 {
            let t1 = Instant::now();
            let (_label, result) = client.core.read(&op, cfg.timeout_ms, &log).await;
            let elapsed = t1.elapsed();
            assert!(elapsed >= Duration::from_millis(300));
            assert!(elapsed < Duration::from_millis(700));
            assert!(matches!(result, Err(crate::ModbusError::Timeout(_))));
        }
    }
}
