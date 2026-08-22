// Crate
use crate::common::serial_config_from;
use crate::monitor::{MonitorEnd, SharedObservedTable, SharedRecordLog, drive_monitor};
use crate::rtu::Config;
use crate::server_core::wait_reconnect_backoff;
use crate::{Error, LogFn, PathConflictCell, SerialError, ServerCommand};

// Workspace
use ferrowl_util::backoff::{AttemptOutcome, BackoffPolicy, run_with_backoff};

// External
use rust_modbus::{AduReader, Direction, Rtu, TransportConfig, open_serial};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus RTU monitor task (MB-R-140–MB-R-145): opens the configured
/// serial port receive-only and decodes whatever traffic passes on the wire into `table`,
/// never itself writing to the port (MB-R-141).
pub struct MonitorBuilder {
    config: Arc<RwLock<Config>>,
    table: SharedObservedTable,
    records: SharedRecordLog,
    path_conflict: PathConflictCell,
}

impl MonitorBuilder {
    pub fn new(
        config: Arc<RwLock<Config>>,
        table: SharedObservedTable,
        records: SharedRecordLog,
    ) -> Self {
        Self {
            config,
            table,
            records,
            path_conflict: PathConflictCell::default(),
        }
    }

    /// MB-R-150 — see `rtu::ClientBuilder::path_conflict` (same late-binding contract).
    pub fn path_conflict(&self) -> PathConflictCell {
        self.path_conflict.clone()
    }

    /// Spawns the receive loop as a tokio task and always returns `Ok` (mirrors
    /// `rtu::ServerBuilder::spawn`, MB-R-130): a bad path or busy port surfaces from the
    /// joined `JoinHandle` after exhausting retries (`reconnect: false`) or never, if
    /// `reconnect` stays true and the caller eventually sends `ServerCommand::Terminate`.
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
        let table = self.table.clone();
        let records = self.records.clone();
        let path_conflict = self.path_conflict.clone();
        Ok(tokio::task::spawn(run(
            config,
            table,
            records,
            receiver,
            log,
            status,
            path_conflict,
        )))
    }
}

/// MB-R-141 — open the configured serial port receive-only and drive the monitor's decode/
/// match loop, retrying the open with the shared backoff policy on failure (MB-R-130–134).
async fn run<L, St>(
    config: Arc<RwLock<Config>>,
    table: SharedObservedTable,
    records: SharedRecordLog,
    receiver: Receiver<ServerCommand>,
    log: L,
    status: St,
    path_conflict: PathConflictCell,
) -> Result<(), Error>
where
    L: LogFn + Clone,
    St: LogFn + Clone,
{
    let receiver = AsyncMutex::new(receiver);
    let activity = Arc::new(AtomicBool::new(false));

    let attempt = || {
        let config = config.clone();
        let table = table.clone();
        let records = records.clone();
        let log = log.clone();
        let activity = activity.clone();
        let receiver = &receiver;
        let path_conflict = path_conflict.clone();
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
                        reconnect: false,
                        reset: false,
                    };
                }
            };
            let path = guard.path.clone();
            drop(guard);
            // MB-R-150 — check the freshly `~`-expanded path for a conflict before the OS-level
            // open; a conflict is a `Failed` outcome carrying the ordinary reconnect setting.
            let expanded = ferrowl_util::path::expand(&path);
            let expanded = expanded.to_string_lossy().into_owned();
            if let Some(other) = path_conflict.check(&expanded) {
                return AttemptOutcome::Failed {
                    error: Error::PathConflict {
                        path: expanded,
                        other,
                    },
                    reconnect,
                    reset: false,
                };
            }
            let transport_config = match TransportConfig::from_serial(&serial) {
                Ok(cfg) => cfg,
                Err(e) => {
                    return AttemptOutcome::Failed {
                        error: SerialError::Error(e).into(),
                        reconnect: false,
                        reset: false,
                    };
                }
            };
            match open_serial::<Rtu>(&path, serial) {
                Err(e) => AttemptOutcome::Failed {
                    error: SerialError::Error(e).into(),
                    reconnect,
                    reset: false,
                },
                Ok(transport) => {
                    let stream = transport.into_inner();
                    let reader = AduReader::<_, Rtu>::with_config(
                        stream,
                        Direction::Request,
                        transport_config,
                    );
                    let mut receiver = receiver.lock().await;
                    match drive_monitor::<_, Rtu, _>(
                        reader,
                        log.clone(),
                        table.clone(),
                        records.clone(),
                        &activity,
                        &mut receiver,
                    )
                    .await
                    {
                        MonitorEnd::Terminated => AttemptOutcome::Done,
                        MonitorEnd::Failed(e) => AttemptOutcome::Failed {
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
    status.invoke("Monitor stopped".to_string()).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathConflictCell;
    use crate::monitor::{ObservedTable, RecordLog};
    use tokio::sync::mpsc;

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
    async fn ut_rtu_monitor_run_reports_path_conflict_and_skips_open() {
        let cfg = Arc::new(RwLock::new(config("/nonexistent/mb-r-150-ut-rtu-monitor")));
        let table: SharedObservedTable =
            Arc::new(parking_lot::RwLock::new(ObservedTable::default()));
        let records: SharedRecordLog = Arc::new(parking_lot::RwLock::new(RecordLog::default()));
        let (_tx, rx) = mpsc::channel(1);
        let path_conflict = PathConflictCell::default();
        path_conflict.set(std::sync::Arc::new(|_: &str| {
            Some("other-module".to_string())
        }));

        let result = run(cfg, table, records, rx, sink(), sink(), path_conflict).await;

        assert!(
            matches!(result, Err(Error::PathConflict { .. })),
            "expected Err(Error::PathConflict), got {result:?}"
        );
    }
}
