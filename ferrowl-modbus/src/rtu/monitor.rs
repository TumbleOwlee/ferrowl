// Crate
use crate::common::serial_config_from;
use crate::monitor::{MonitorEnd, SharedObservedTable, drive_monitor};
use crate::rtu::Config;
use crate::server_core::wait_reconnect_backoff;
use crate::{Error, LogFn, SerialError, ServerCommand};

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
}

impl MonitorBuilder {
    pub fn new(config: Arc<RwLock<Config>>, table: SharedObservedTable) -> Self {
        Self { config, table }
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
        Ok(tokio::task::spawn(run(
            config, table, receiver, log, status,
        )))
    }
}

/// MB-R-141 — open the configured serial port receive-only and drive the monitor's decode/
/// match loop, retrying the open with the shared backoff policy on failure (MB-R-130–134).
async fn run<L, St>(
    config: Arc<RwLock<Config>>,
    table: SharedObservedTable,
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
