// Crate
use crate::common::serial_config_from;
use crate::monitor::{MonitorEnd, SharedObservedTable, SharedRecordLog, drive_monitor};
use crate::rtu::Config;
use crate::server_core::wait_reconnect_backoff;
use crate::{Error, LogFn, SerialError, ServerCommand};

// Workspace
use ferrowl_util::backoff::{AttemptOutcome, BackoffPolicy, run_with_backoff};

// External
use rust_modbus::{AduReader, Ascii, Direction, TransportConfig, open_serial};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus ASCII monitor task (MB-R-140–MB-R-145); same shape as
/// `rtu::monitor::MonitorBuilder`, differing only in on-wire framing.
pub struct MonitorBuilder {
    config: Arc<RwLock<Config>>,
    table: SharedObservedTable,
    records: SharedRecordLog,
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
        }
    }

    /// See `rtu::monitor::MonitorBuilder::spawn`.
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
        Ok(tokio::task::spawn(run(
            config, table, records, receiver, log, status,
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
            match open_serial::<Ascii>(&path, serial) {
                Err(e) => AttemptOutcome::Failed {
                    error: SerialError::Error(e).into(),
                    reconnect,
                    reset: false,
                },
                Ok(transport) => {
                    let stream = transport.into_inner();
                    let reader = AduReader::<_, Ascii>::with_config(
                        stream,
                        Direction::Request,
                        transport_config,
                    );
                    let mut receiver = receiver.lock().await;
                    match drive_monitor::<_, Ascii, _>(
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
