// Crate
use crate::server_core::{ResetOn, ServeEnd, Server, drive_serve, wait_reconnect_backoff};
use crate::tcp::Config;
use crate::tcp::tls::build_server_tls_config;
use crate::{Error, Key, KeyParams, LogFn, ServerCommand, TcpError};

// Workspace
use ferrowl_store::Memory;
use ferrowl_util::backoff::{AttemptOutcome, BackoffPolicy, run_with_backoff};

// External
use parking_lot::RwLock as MemLock;
use rust_modbus::{Server as ModbusServer, TcpListener, TlsListener};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
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

    /// Binds the configured listen address and spawns the accept loop as a tokio task. `log`
    /// receives log lines, `status` receives a "Server stopped" line once the task ends, and
    /// `receiver` delivers `ServerCommand::Terminate`.
    ///
    /// `spawn` itself always returns `Ok` — a bind or serve failure no longer fails the start
    /// synchronously; it surfaces from the returned `JoinHandle` instead (MB-R-130/MB-R-134).
    /// With `config.reconnect` set (the default), a bind failure or a mid-serve failure does not
    /// end the task: it logs, waits an exponential backoff (capped, reset after a serve loop
    /// that accepted at least one connection), and retries (MB-R-114, MB-R-130–134). A TLS
    /// configuration error (MB-R-107/MB-R-108) always ends the task immediately regardless of
    /// `reconnect` — retrying an invalid configuration can never succeed.
    /// `ServerCommand::Terminate` (or the channel closing) aborts a backoff wait immediately
    /// (MB-R-133).
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

/// Every production server now logs per-request outcomes (MB-R-067); RtuOverTcp is no
/// exception.
const VERBOSE: bool = true;

/// MB-R-128 — this transport is never physical Rtu/Ascii serial (RtuOverTcp/AsciiOverTcp ride
/// TCP; Tcp and Udp have no serial concept at all): an unmapped slave id keeps the ordinary
/// exception, same as MB-R-065/MB-R-060.
const PHYSICAL_SERIAL: bool = false;

/// Drive the retry loop: bind the configured TCP address and serve using RTU framing
/// (MB-R-113), retrying a bind or mid-serve failure per [`BackoffPolicy`] when
/// `config.reconnect` is set (MB-R-114, MB-R-130–134). Each accepted connection answers from
/// the shared `memory` via a [`Server`] (verbose logging on, MB-R-067). Plain TCP unless
/// `config.tls` is set (MB-R-115), in which case the listener terminates TLS on each accepted
/// connection.
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
    // Shared between `attempt` and `wait_abortable`, called strictly sequentially by
    // `run_with_backoff` and never concurrently — same technique as
    // `client_core::run_reconnect_loop` (see Shared).
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
                        // A malformed address never fixes itself by retrying; matches the
                        // pre-retry behavior, which failed this unconditionally too.
                        reconnect: false,
                        reset: false,
                    };
                }
            };
            let server = ModbusServer::new(
                Server::new(memory.clone(), log.clone(), VERBOSE, PHYSICAL_SERIAL)
                    .with_reset_on(activity.clone(), ResetOn::Connect),
            );
            match &guard.tls {
                None => {
                    drop(guard);
                    match TcpListener::bind(addr).await {
                        Err(e) => AttemptOutcome::Failed {
                            error: Error::Server(e),
                            reconnect,
                            reset: false,
                        },
                        Ok(listener) => {
                            let handle = server.handle();
                            let mut receiver = receiver.lock().await;
                            match drive_serve(
                                server.serve_framed::<rust_modbus::RtuOverTcp>(listener),
                                handle,
                                &mut receiver,
                            )
                            .await
                            {
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
                Some(tls) => {
                    let build_result = build_server_tls_config(tls, &guard.ip);
                    drop(guard);
                    match build_result {
                        Err(e) => AttemptOutcome::Failed {
                            error: Error::Tcp(e),
                            // A TLS configuration error never fixes itself by retrying
                            // (MB-R-107/MB-R-108) — always ends the task, regardless of
                            // `reconnect`.
                            reconnect: false,
                            reset: false,
                        },
                        Ok((tls_config, used_fallback)) => {
                            if used_fallback {
                                log.invoke(
                                    "No cert_file/key_file/self_signed configured for this TLS \
                                     server; falling back to an ephemeral self-signed certificate."
                                        .to_string(),
                                )
                                .await;
                            }
                            match TlsListener::bind(addr, tls_config).await {
                                Err(e) => AttemptOutcome::Failed {
                                    error: Error::Server(e),
                                    reconnect,
                                    reset: false,
                                },
                                Ok(listener) => {
                                    let handle = server.handle();
                                    let mut receiver = receiver.lock().await;
                                    match drive_serve(
                                        server.serve_tls::<rust_modbus::RtuOverTcp>(listener),
                                        handle,
                                        &mut receiver,
                                    )
                                    .await
                                    {
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

    /// MB-R-067 — the RtuOverTcp server logs per-request outcomes exactly like every other
    /// transport (RTU, RTU-over-TCP, TCP alike now).
    #[test]
    fn ut_rtu_over_tcp_server_is_verbose() {
        assert!(VERBOSE);
    }

    /// MB-R-128 — RtuOverTcp is never physical-serial: an unmapped slave id keeps the ordinary
    /// exception.
    #[test]
    fn ut_rtu_over_tcp_server_is_not_physical_serial() {
        assert!(!PHYSICAL_SERIAL);
    }
}
