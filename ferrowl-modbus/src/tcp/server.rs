// Crate
use crate::server_core::{
    BoundAddr, ResetOn, ServeEnd, Server, drive_serve, wait_reconnect_backoff,
};
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

/// Builds and spawns a Modbus TCP server task answering requests from the
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
    /// that accepted at least one connection), and retries (MB-R-071 revised, MB-R-130–134). A
    /// TLS configuration error (MB-R-107/MB-R-108) always ends the task immediately regardless
    /// of `reconnect` — retrying an invalid configuration can never succeed.
    /// `ServerCommand::Terminate` (or the channel closing) aborts a backoff wait immediately
    /// (MB-R-133).
    ///
    /// The returned `BoundAddr` is `None` until the listener actually binds and clears again
    /// once its serve loop ends — a caller that needs to know the listener is up (rather than
    /// merely that the task was scheduled) polls it instead of racing `spawn()`'s return with a
    /// fixed sleep (see [`BoundAddr`]).
    pub async fn spawn<L, St>(
        &self,
        receiver: Receiver<ServerCommand>,
        log: L,
        status: St,
    ) -> Result<(JoinHandle<Result<(), Error>>, BoundAddr), Error>
    where
        L: LogFn + Clone,
        St: LogFn + Clone,
    {
        let config = self.config.clone();
        let memory = self.memory.clone();
        let bound_addr: BoundAddr = Arc::new(parking_lot::Mutex::new(None));
        let handle = tokio::task::spawn(run(
            config,
            memory,
            receiver,
            log,
            status,
            bound_addr.clone(),
        ));
        Ok((handle, bound_addr))
    }
}

/// Every production server logs per-request outcomes (MB-R-067); TCP is no exception.
const VERBOSE: bool = true;

/// MB-R-128 — this transport is never physical Rtu/Ascii serial (RtuOverTcp/AsciiOverTcp ride
/// TCP; Tcp and Udp have no serial concept at all): an unmapped slave id keeps the ordinary
/// exception, same as MB-R-065/MB-R-060.
const PHYSICAL_SERIAL: bool = false;

/// Drive the retry loop: bind the configured TCP address and serve, retrying a bind or
/// mid-serve failure per [`BackoffPolicy`] when `config.reconnect` is set (MB-R-071 revised,
/// MB-R-130–134). Each accepted connection answers from the shared `memory` via a [`Server`]
/// (verbose logging on, MB-R-067). Plain TCP unless `config.tls` is set (MB-R-104), in which
/// case the listener terminates TLS on each accepted connection.
async fn run<T, L, St>(
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    receiver: Receiver<ServerCommand>,
    log: L,
    status: St,
    bound_addr: BoundAddr,
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
        let bound_addr = bound_addr.clone();
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
                            let bound = match listener.local_addr() {
                                Ok(addr) => addr,
                                Err(e) => {
                                    return AttemptOutcome::Failed {
                                        error: Error::Server(e),
                                        reconnect,
                                        reset: false,
                                    };
                                }
                            };
                            *bound_addr.lock() = Some(bound);
                            let handle = server.handle();
                            let mut receiver = receiver.lock().await;
                            let end =
                                drive_serve(server.serve(listener), handle, &mut receiver).await;
                            *bound_addr.lock() = None;
                            match end {
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
                                    let bound = match listener.local_addr() {
                                        Ok(addr) => addr,
                                        Err(e) => {
                                            return AttemptOutcome::Failed {
                                                error: Error::Server(e),
                                                reconnect,
                                                reset: false,
                                            };
                                        }
                                    };
                                    *bound_addr.lock() = Some(bound);
                                    let handle = server.handle();
                                    let mut receiver = receiver.lock().await;
                                    let end = drive_serve(
                                        server.serve_tls::<rust_modbus::Tcp>(listener),
                                        handle,
                                        &mut receiver,
                                    )
                                    .await;
                                    *bound_addr.lock() = None;
                                    match end {
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
    use super::{PHYSICAL_SERIAL, ServerBuilder, VERBOSE};
    use crate::tcp::Config;
    use crate::{Key, ServerCommand, SlaveKey};

    /// MB-R-067 — the TCP server logs per-request outcomes exactly like every other transport.
    #[test]
    fn ut_tcp_server_is_verbose() {
        assert!(VERBOSE);
    }

    /// MB-R-128 — Tcp is never physical-serial: an unmapped slave id keeps the ordinary
    /// exception.
    #[test]
    fn ut_tcp_server_is_not_physical_serial() {
        assert!(!PHYSICAL_SERIAL);
    }

    fn sink() -> impl crate::LogFn + Clone {
        |_s: String| async move {}
    }

    /// MB-R-130 (bound_addr companion) — `spawn()` only guarantees the task was scheduled, not
    /// that its first bind attempt has run (the bind moved inside the retried task itself); the
    /// second element of `spawn()`'s return is the ready signal a caller polls instead of racing
    /// it with a fixed sleep. `None` before the first successful bind, `Some(<real addr>)` once
    /// bound (even with the configured `port: 0`), `None` again once `ServerCommand::Terminate`
    /// ends the serve loop.
    #[tokio::test]
    async fn ut_bound_addr_reflects_bind_lifecycle() {
        let config = Config {
            ip: "127.0.0.1".to_string(),
            port: 0,
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
            tls: None,
        };
        let memory = std::sync::Arc::new(parking_lot::RwLock::new(ferrowl_store::Memory::<
            Key<SlaveKey>,
        >::default()));
        let (tx, rx) = tokio::sync::mpsc::channel::<ServerCommand>(1);
        let (handle, bound_addr) = ServerBuilder::new(
            std::sync::Arc::new(tokio::sync::RwLock::new(config)),
            memory,
        )
        .spawn(rx, sink(), sink())
        .await
        .expect("spawn always returns Ok");

        let mut addr = None;
        for _ in 0..50 {
            addr = *bound_addr.lock();
            if addr.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let addr = addr.expect("listener must have bound within 1s");
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_ne!(addr.port(), 0, "the OS must have assigned a real port");

        tx.send(ServerCommand::Terminate).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        assert!(
            bound_addr.lock().is_none(),
            "bound_addr must clear once the serve loop ends"
        );
    }
}
