// Crate
use crate::server_core::{ResetOn, ServeEnd, Server, drive_serve, wait_reconnect_backoff};
use crate::udp::Config;
use crate::{Error, Key, KeyParams, LogFn, ServerCommand, TcpError};

// Workspace
use ferrowl_store::Memory;
use ferrowl_util::backoff::{AttemptOutcome, BackoffPolicy, run_with_backoff};

// External
use parking_lot::RwLock as MemLock;
use rust_modbus::Server as ModbusServer;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::UdpSocket;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus UDP server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    /// Spawns the serve loop as a tokio task and always returns `Ok` (MB-R-130): the UDP bind
    /// moves inside the retried task itself, so a bind failure no longer fails `spawn()`
    /// synchronously — it surfaces from the joined `JoinHandle` instead, after exhausting
    /// retries (`reconnect: false`, MB-R-134) or never, if `reconnect` stays true and the
    /// caller eventually sends `ServerCommand::Terminate` (MB-R-133). `log` receives log
    /// lines, `status` receives lifecycle status lines.
    pub async fn spawn<L, St>(
        &self,
        receiver: Receiver<ServerCommand>,
        log: L,
        status: St,
    ) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
        St: LogFn + Clone,
    {
        let config = self.config.clone();
        let memory = self.memory.clone();
        Ok(tokio::task::spawn(run(
            config, memory, receiver, log, status,
        )))
    }
}

/// Every production server logs per-request outcomes (MB-R-067); UDP is no exception.
const VERBOSE: bool = true;

/// MB-R-128 — this transport is never physical Rtu/Ascii serial (RtuOverTcp/AsciiOverTcp ride
/// TCP; Tcp and Udp have no serial concept at all): an unmapped slave id keeps the ordinary
/// exception, same as MB-R-065/MB-R-060.
const PHYSICAL_SERIAL: bool = false;

/// Bind the configured UDP address and serve it, retrying the bind with the shared backoff
/// policy on failure (MB-R-120 revised, MB-R-130–134); each datagram answers from the shared
/// `memory` via a [`Server`] (verbose logging on, MB-R-067, mirroring the TCP server). No
/// accept loop, no TLS branch (MB-R-116 — `udp::Config` carries no `tls` field at all).
/// `ResetOn::Request`: `serve_udp` never calls `on_connect` at all (no connection concept for
/// UDP), so reading a datagram is the only "did something useful" signal (see Shared note in
/// plan.md).
async fn run<T, L, St>(
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    receiver: Receiver<ServerCommand>,
    log: L,
    status: St,
) -> Result<(), Error>
where
    T: KeyParams,
    L: LogFn + Clone,
    St: LogFn + Clone,
{
    let receiver = AsyncMutex::new(receiver);
    let activity = Arc::new(AtomicBool::new(false));

    let attempt = || {
        let config = config.clone();
        let memory = memory.clone();
        let log = log.clone();
        let activity = activity.clone();
        let receiver = &receiver;
        async move {
            activity.store(false, Ordering::Relaxed);
            let guard = config.read().await;
            let reconnect = guard.reconnect;
            let addr: SocketAddr = match format!("{}:{}", guard.ip, guard.port).parse() {
                Ok(addr) => addr,
                Err(e) => {
                    return AttemptOutcome::Failed {
                        error: Error::Tcp(TcpError::Address(e)),
                        reconnect: false, // config errors never retry
                        reset: false,
                    };
                }
            };
            drop(guard);
            match UdpSocket::bind(addr).await {
                Err(e) => AttemptOutcome::Failed {
                    error: Error::Server(e.into()),
                    reconnect,
                    reset: false,
                },
                Ok(socket) => {
                    let server = ModbusServer::new(
                        Server::new(memory.clone(), log.clone(), VERBOSE, PHYSICAL_SERIAL)
                            .with_reset_on(activity.clone(), ResetOn::Request),
                    );
                    let handle = server.handle();
                    let mut receiver = receiver.lock().await;
                    match drive_serve(server.serve_udp(socket), handle, &mut receiver).await {
                        ServeEnd::Terminated => AttemptOutcome::Done,
                        ServeEnd::Failed(e) => AttemptOutcome::Failed {
                            error: Error::Server(e),
                            reconnect,
                            reset: activity.load(Ordering::Relaxed),
                        },
                    }
                }
            }
        }
    };

    let wait_abortable = |backoff: std::time::Duration| {
        let receiver = &receiver;
        async move {
            let mut receiver = receiver.lock().await;
            wait_reconnect_backoff(&mut receiver, backoff).await
        }
    };

    let result = run_with_backoff(BackoffPolicy::default(), attempt, wait_abortable).await;
    status.invoke("Server stopped".to_string()).await;
    result
}

#[cfg(test)]
mod tests {
    use super::{PHYSICAL_SERIAL, VERBOSE};

    /// MB-R-067 — the UDP server logs per-request outcomes exactly like every other transport.
    #[test]
    fn ut_udp_server_is_verbose() {
        assert!(VERBOSE);
    }

    /// MB-R-128 — Udp is never physical-serial: an unmapped slave id keeps the ordinary
    /// exception.
    #[test]
    fn ut_udp_server_is_not_physical_serial() {
        assert!(!PHYSICAL_SERIAL);
    }
}
