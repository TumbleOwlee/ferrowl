use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::server_core::{ResetOn, ServeEnd, Server, drive_serve, wait_reconnect_backoff};
use crate::{
    ConnectedCell, Error, Key, KeyParams, LogFn, PathConflictCell, SerialError, ServerCommand,
};

use ferrowl_store::Memory;
use ferrowl_util::backoff::{AttemptOutcome, BackoffPolicy, run_with_backoff};

use parking_lot::RwLock as MemLock;
use rust_modbus::{Ascii, Server as ModbusServer, open_serial};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus ASCII server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    path_conflict: PathConflictCell,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self {
            config,
            memory,
            path_conflict: PathConflictCell::default(),
        }
    }

    /// MB-R-150 — see `rtu::ClientBuilder::path_conflict` (same late-binding contract).
    pub fn path_conflict(&self) -> PathConflictCell {
        self.path_conflict.clone()
    }

    /// Spawns the serve loop as a tokio task and always returns `Ok` (MB-R-130): the serial
    /// port open moves inside the retried task itself, so a bad path or busy port no longer
    /// fails `spawn()` synchronously — it surfaces from the joined `JoinHandle` instead, after
    /// exhausting retries (`reconnect: false`, MB-R-134) or never, if `reconnect` stays true
    /// and the caller eventually sends `ServerCommand::Terminate` (MB-R-133). `log` receives
    /// log lines, `status` receives lifecycle status lines.
    pub async fn spawn<L, St>(
        &self,
        receiver: Receiver<ServerCommand>,
        log: L,
        status: St,
    ) -> Result<(JoinHandle<Result<(), Error>>, ConnectedCell), Error>
    where
        L: LogFn + Clone,
        St: LogFn + Clone,
    {
        let config = self.config.clone();
        let memory = self.memory.clone();
        let path_conflict = self.path_conflict.clone();
        let open = ConnectedCell::default();
        let handle = tokio::task::spawn(run(
            config,
            memory,
            receiver,
            log,
            status,
            path_conflict,
            open.clone(),
        ));
        Ok((handle, open))
    }
}

/// Every production server logs per-request outcomes (MB-R-067); Ascii is no exception.
const VERBOSE: bool = true;

/// MB-R-128 — this is a physical Rtu/Ascii serial link: an unmapped slave id is answered with
/// silence, not an exception.
const PHYSICAL_SERIAL: bool = true;

/// Open the configured serial port and serve it, retrying the open with the shared backoff
/// policy on failure (MB-R-124 revised, MB-R-130–134). `ResetOn::Request` (not `Connect`): the
/// Ascii link's own `on_connect` fires once immediately, before any request is read, so it
/// cannot be the "did something useful" signal — only reading a request/datagram counts.
async fn run<T, L, St>(
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    receiver: Receiver<ServerCommand>,
    log: L,
    status: St,
    path_conflict: PathConflictCell,
    open: ConnectedCell,
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
        let path_conflict = path_conflict.clone();
        let open = open.clone();
        async move {
            activity.store(false, Ordering::Relaxed);
            let guard = config.read().await;
            let reconnect = guard.reconnect;
            let serial = match serial_config_from(
                guard.baud_rate,
                guard.data_bits,
                guard.stop_bits,
                guard.parity.as_deref(),
            ) {
                Ok(serial) => serial,
                Err(e) => {
                    return AttemptOutcome::Failed {
                        error: e.into(),
                        reconnect: false, // a bad serial-config value never fixes itself
                        reset: false,
                    };
                }
            };
            let path = guard.path.clone();
            drop(guard);
            // MB-R-150 — check the freshly `~`-expanded path for a conflict before the OS-level
            // open; a conflict is a `Failed` outcome carrying the ordinary reconnect setting, so
            // it retries on the usual backoff and recovers once the conflict clears.
            let expanded = ferrowl_util::path::expand(&path);
            let expanded = expanded.to_string_lossy().into_owned();
            if let Some(other) = path_conflict.check(&expanded) {
                log.invoke(format!(
                    "Serial path '{expanded}' is already in use by module '{other}' in this \
                     session; skipping open."
                ))
                .await;
                return AttemptOutcome::Failed {
                    error: Error::PathConflict {
                        path: expanded,
                        other,
                    },
                    reconnect,
                    reset: false,
                };
            }
            match open_serial::<Ascii>(&path, serial) {
                Err(e) => AttemptOutcome::Failed {
                    error: SerialError::Error(e).into(),
                    reconnect,
                    reset: false,
                },
                Ok(transport) => {
                    open.set(true);
                    let server = ModbusServer::new(
                        Server::new(memory.clone(), log.clone(), VERBOSE, PHYSICAL_SERIAL)
                            .with_reset_on(activity.clone(), ResetOn::Request),
                    );
                    let handle = server.handle();
                    let mut receiver = receiver.lock().await;
                    let end =
                        drive_serve(server.serve_link(transport), handle, &mut receiver).await;
                    open.set(false);
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
    use super::*;
    use crate::{Key, PathConflictCell, SlaveKey};
    use ferrowl_store::Memory;
    use tokio::sync::mpsc;

    /// MB-R-067 — the Ascii server logs per-request outcomes exactly like every
    /// other transport.
    #[test]
    fn ut_ascii_server_is_verbose() {
        const { assert!(VERBOSE) };
    }

    /// MB-R-128 — the Ascii server is wired as physical-serial: an unmapped slave id is answered
    /// with silence.
    #[test]
    fn ut_ascii_server_is_physical_serial() {
        const { assert!(PHYSICAL_SERIAL) };
    }

    fn sink() -> impl crate::LogFn + Clone {
        |_s: String| async move {}
    }

    fn config(path: &str) -> Config {
        Config {
            path: path.to_string(),
            baud_rate: 9600,
            slave: 0,
            parity: None,
            data_bits: None,
            stop_bits: None,
            timeout_ms: 100,
            delay_ms: 0,
            interval_ms: 100,
            reconnect: false,
        }
    }

    /// MB-R-150 — a path-conflict checker attached to the builder's cell makes `run()` end the
    /// (non-reconnecting) attempt with `Error::PathConflict`, without ever calling
    /// `open_serial`.
    #[tokio::test]
    async fn ut_ascii_server_run_reports_path_conflict_and_skips_open() {
        let cfg = Arc::new(RwLock::new(config("/nonexistent/mb-r-150-ut-ascii-server")));
        let memory: Arc<MemLock<Memory<Key<SlaveKey>>>> = Arc::new(MemLock::new(Memory::default()));
        let (_tx, rx) = mpsc::channel(1);
        let path_conflict = PathConflictCell::default();
        path_conflict.set(std::sync::Arc::new(|_: &str| {
            Some("other-module".to_string())
        }));

        let result = run::<SlaveKey, _, _>(
            cfg,
            memory,
            rx,
            sink(),
            sink(),
            path_conflict,
            ConnectedCell::default(),
        )
        .await;

        assert!(
            matches!(result, Err(Error::PathConflict { .. })),
            "expected Err(Error::PathConflict), got {result:?}"
        );
    }

    /// MB-R-150 — "report a distinct path-conflict status/log entry instead — replacing today's
    /// silent indefinite retry" (Shared, mirrors the RTU server's own requirement).
    #[tokio::test]
    async fn ut_ascii_server_run_logs_path_conflict_before_returning() {
        use std::sync::Mutex;

        let cfg = Arc::new(RwLock::new(config(
            "/nonexistent/mb-r-150-ut-ascii-server-log",
        )));
        let memory: Arc<MemLock<Memory<Key<SlaveKey>>>> = Arc::new(MemLock::new(Memory::default()));
        let (_tx, rx) = mpsc::channel(1);
        let path_conflict = PathConflictCell::default();
        path_conflict.set(std::sync::Arc::new(|_: &str| {
            Some("other-module".to_string())
        }));
        let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_sink = {
            let lines = lines.clone();
            move |s: String| {
                let lines = lines.clone();
                async move {
                    lines.lock().unwrap().push(s);
                }
            }
        };

        let _ = run::<SlaveKey, _, _>(
            cfg,
            memory,
            rx,
            log_sink,
            sink(),
            path_conflict,
            ConnectedCell::default(),
        )
        .await;

        let logged = lines.lock().unwrap();
        assert!(
            logged
                .iter()
                .any(|l| l.contains("already in use by module 'other-module'")),
            "expected a path-conflict log line, got: {logged:?}"
        );
    }

    /// MB-R-153 — a serial-open attempt that never succeeds (a bad path, `reconnect: false` so
    /// the task ends after one attempt) never flips the open cell true. A real "flips true while
    /// serving" assertion needs an actual openable serial device, which this crate's own
    /// `tests/ascii_serial.rs` documents as unavailable in CI (no portable named-PTY loopback);
    /// this is the CI-portable half of MB-R-153's contract.
    #[tokio::test]
    async fn ut_ascii_server_open_cell_stays_false_through_failed_attempt() {
        let cfg = Arc::new(RwLock::new(config(
            "/nonexistent/mb-r-153-ut-ascii-server-open-cell",
        )));
        let memory: Arc<MemLock<Memory<Key<SlaveKey>>>> = Arc::new(MemLock::new(Memory::default()));
        let (_tx, rx) = mpsc::channel(1);
        let open = ConnectedCell::default();

        let result = run::<SlaveKey, _, _>(
            cfg,
            memory,
            rx,
            sink(),
            sink(),
            PathConflictCell::default(),
            open.clone(),
        )
        .await;

        assert!(
            result.is_err(),
            "a bad path with reconnect: false ends the task with an error"
        );
        assert!(
            !open.get(),
            "the open cell must never observe true when the port was never opened"
        );
    }
}
