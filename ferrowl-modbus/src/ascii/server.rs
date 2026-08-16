// Crate
use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::server_core::{ResetOn, ServeEnd, Server, drive_serve, wait_reconnect_backoff};
use crate::{Error, Key, KeyParams, LogFn, SerialError, ServerCommand};

// Workspace
use ferrowl_store::Memory;
use ferrowl_util::backoff::{AttemptOutcome, BackoffPolicy, run_with_backoff};

// External
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
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
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

/// Every production server logs per-request outcomes (MB-R-067); Ascii is no exception.
const VERBOSE: bool = true;

/// MB-R-128 — this is a physical Rtu/Ascii serial link: an unmapped slave id is answered with
/// silence, not an exception.
const PHYSICAL_SERIAL: bool = true;

/// Open the configured serial port and serve it, retrying the open with the shared backoff
/// policy on failure (MB-R-124 revised, MB-R-130–134). `ResetOn::Request` (not `Connect`): the
/// Ascii link's own `on_connect` fires once immediately, before any request is read, so it
/// cannot be the "did something useful" signal — only reading a request/datagram counts (see
/// Shared note in plan.md).
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
            match open_serial::<Ascii>(&path, serial) {
                Err(e) => AttemptOutcome::Failed {
                    error: SerialError::Error(e).into(),
                    reconnect,
                    reset: false,
                },
                Ok(transport) => {
                    let server = ModbusServer::new(
                        Server::new(memory.clone(), log.clone(), VERBOSE, PHYSICAL_SERIAL)
                            .with_reset_on(activity.clone(), ResetOn::Request),
                    );
                    let handle = server.handle();
                    let mut receiver = receiver.lock().await;
                    match drive_serve(server.serve_link(transport), handle, &mut receiver).await {
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

    /// MB-R-067 — the Ascii server logs per-request outcomes exactly like every
    /// other transport.
    #[test]
    fn ut_ascii_server_is_verbose() {
        assert!(VERBOSE);
    }

    /// MB-R-128 — the Ascii server is wired as physical-serial: an unmapped slave id is answered
    /// with silence.
    #[test]
    fn ut_ascii_server_is_physical_serial() {
        assert!(PHYSICAL_SERIAL);
    }
}
