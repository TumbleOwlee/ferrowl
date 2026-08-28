use crate::client_core::{ClientCore, ConnectAttempt, connect_serial};
use crate::rtu::Config;
use crate::{Command, ConnectedCell, Error, Key, KeyParams, LogFn, Operation, PathConflictCell};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use tokio::task::JoinHandle;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

use rust_modbus::{Ascii, FrameTransport, SerialStream};

/// Builds and spawns a Modbus ASCII client task that polls `operations` into
/// the shared `memory` and executes incoming [`Command`]s.
pub struct ClientBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    path_conflict: PathConflictCell,
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
            path_conflict: PathConflictCell::default(),
        }
    }

    /// MB-R-150 — see [`PathConflictCell`] for the late-binding contract.
    pub fn path_conflict(&self) -> PathConflictCell {
        self.path_conflict.clone()
    }

    /// Opens the serial port and spawns the client loop as a tokio task. `log` receives log
    /// lines, `status` receives connection status updates, and `receiver` delivers
    /// write/terminate [`Command`]s.
    ///
    /// With `config.reconnect` set (the default), a lost or unopenable port does not end the
    /// task: it logs, waits an exponential backoff (capped, reset after a run that got at least
    /// one read through), and retries. `Command::Terminate` (or the channel closing) aborts a
    /// backoff wait immediately. With `config.reconnect` unset, a transport error ends the task
    /// exactly as before this behavior was added.
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
        let config = self.config.clone();
        let operations = self.operations.clone();
        let memory = self.memory.clone();
        let path_conflict = self.path_conflict.clone();
        let connected = ConnectedCell::default();
        let connected_for_task = connected.clone();
        let handle = tokio::task::spawn(async move {
            ClientCore::run_reconnect_loop(
                receiver,
                log,
                status,
                operations,
                memory,
                move || {
                    let config = config.clone();
                    let path_conflict = path_conflict.clone();
                    async move {
                        let guard = config.read().await;
                        let attempt = ConnectAttempt {
                            reconnect: guard.reconnect,
                            timeout_ms: guard.timeout_ms,
                            delay_ms: guard.delay_ms,
                            interval_ms: guard.interval_ms,
                            client: Client::connect(&guard, &path_conflict)
                                .await
                                .map(|client| client.core),
                        };
                        drop(guard);
                        attempt
                    }
                },
                connected_for_task,
            )
            .await
        });
        Ok((handle, connected))
    }
}

/// A connected Modbus ASCII client. Connection setup is serial-specific; the read/command loop
/// is shared via the internal `ClientCore`, over a serial port carrying ASCII framing.
pub struct Client {
    pub(crate) core: ClientCore<FrameTransport<SerialStream, Ascii>, Ascii>,
}

impl Client {
    /// Opens the configured serial port under ASCII framing; see `connect_serial` for the
    /// MB-R-122/MB-R-150 connect contract shared with `rtu::Client::connect`.
    pub async fn connect(config: &Config, path_conflict: &PathConflictCell) -> Result<Self, Error> {
        connect_serial::<Ascii>(config, path_conflict).map(|core| Self { core })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathConflictCell;

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

    /// MB-R-150 — a path-conflict checker attached to the cell short-circuits `Client::connect`
    /// with `Error::PathConflict` before any OS-level `open_serial` attempt.
    #[tokio::test]
    async fn ut_client_connect_reports_path_conflict_before_open_attempt() {
        let cfg = config("/nonexistent/mb-r-150-ut-ascii-client");
        let cell = PathConflictCell::default();
        cell.set(std::sync::Arc::new(|_: &str| {
            Some("other-module".to_string())
        }));

        let result = Client::connect(&cfg, &cell).await;
        assert!(
            matches!(result, Err(Error::PathConflict { .. })),
            "expected Err(Error::PathConflict), got an Ok(Client) or a different error variant"
        );
    }
}
