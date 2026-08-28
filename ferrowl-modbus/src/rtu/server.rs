use crate::rtu::Config;
use crate::server_core::run_serial_family;
use crate::{ConnectedCell, Error, Key, KeyParams, LogFn, PathConflictCell, ServerCommand};

use ferrowl_store::Memory;

use parking_lot::RwLock as MemLock;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus RTU server task answering requests from the
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

    /// MB-R-150 — see [`PathConflictCell`] for the late-binding contract.
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

/// Every production server now logs per-request outcomes (MB-R-067); RTU is no longer the
/// quiet exception it used to be.
const VERBOSE: bool = true;

/// MB-R-128 — this is a physical Rtu/Ascii serial link: an unmapped slave id is answered with
/// silence, not an exception.
const PHYSICAL_SERIAL: bool = true;

/// Open the configured serial port and serve it under RTU framing (MB-R-075 revised). Thin
/// wrapper over [`run_serial_family`](crate::server_core::run_serial_family), shared with
/// `ascii::server` — the two transports differ only in which framing they pass.
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
    run_serial_family::<T, rust_modbus::Rtu, L, St>(
        config,
        memory,
        receiver,
        log,
        status,
        path_conflict,
        open,
        VERBOSE,
        PHYSICAL_SERIAL,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, PathConflictCell, SlaveKey};
    use ferrowl_store::Memory;
    use tokio::sync::mpsc;

    /// MB-R-067 — the RTU server logs per-request outcomes exactly like every
    /// other transport now; there is no more quiet-RTU special case.
    #[test]
    fn ut_rtu_server_is_verbose() {
        const { assert!(VERBOSE) };
    }

    /// MB-R-128 — the Rtu server is wired as physical-serial: an unmapped slave id is answered
    /// with silence.
    #[test]
    fn ut_rtu_server_is_physical_serial() {
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
    async fn ut_rtu_server_run_reports_path_conflict_and_skips_open() {
        let cfg = Arc::new(RwLock::new(config("/nonexistent/mb-r-150-ut-rtu-server")));
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
    /// silent indefinite retry": unlike an ordinary open failure (which the server's own
    /// `attempt` closure never logs, only client/monitor do via their reconnect loops), a
    /// conflict must be visible via `log` before the attempt returns, since a `reconnect:true`
    /// server would otherwise retry it forever with no observable trace at all.
    #[tokio::test]
    async fn ut_rtu_server_run_logs_path_conflict_before_returning() {
        use std::sync::Mutex;

        let cfg = Arc::new(RwLock::new(config(
            "/nonexistent/mb-r-150-ut-rtu-server-log",
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
    /// `tests/rtu_serial.rs` documents as unavailable in CI (no portable named-PTY loopback); this
    /// is the CI-portable half of MB-R-153's contract.
    #[tokio::test]
    async fn ut_rtu_server_open_cell_stays_false_through_failed_attempt() {
        let cfg = Arc::new(RwLock::new(config(
            "/nonexistent/mb-r-153-ut-rtu-server-open-cell",
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
