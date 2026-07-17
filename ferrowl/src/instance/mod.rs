//! Lifecycle wrapper around a single Modbus client or server task.

pub mod builder;
pub mod config;
pub mod error;
pub mod handle;

use builder::Builder;
use config::{ClientConfig, ServerConfig};
use error::{Error, InstanceError};
use handle::Handle;

use ferrowl_modbus::{KeyParams, LogFn};

/// A startable/stoppable Modbus endpoint (TCP/RTU x client/server).
///
/// Construct with one of the `with_*` constructors, then [`start`](Self::start)
/// to spawn the background task and [`stop`](Self::stop) to terminate it.
/// The same instance can be restarted after it stops.
pub struct Instance<T: KeyParams> {
    builder: Builder<T>,
    handle: Option<Handle>,
}

impl<T: KeyParams> Instance<T> {
    pub fn active(&self) -> bool {
        if let Some(h) = &self.handle {
            !h.is_finished()
        } else {
            false
        }
    }

    pub fn with_tcp_client(config: ClientConfig<T, ferrowl_modbus::tcp::Config>) -> Self {
        Self {
            builder: Builder::TcpClient(ferrowl_modbus::tcp::ClientBuilder::new(
                config.config,
                config.operations,
                config.memory,
            )),
            handle: None,
        }
    }

    pub fn with_rtu_client(config: ClientConfig<T, ferrowl_modbus::rtu::Config>) -> Self {
        Self {
            builder: Builder::RtuClient(ferrowl_modbus::rtu::ClientBuilder::new(
                config.config,
                config.operations,
                config.memory,
            )),
            handle: None,
        }
    }

    pub fn with_tcp_server(config: ServerConfig<T, ferrowl_modbus::tcp::Config>) -> Self {
        Self {
            builder: Builder::TcpServer(ferrowl_modbus::tcp::ServerBuilder::new(
                config.config,
                config.memory,
            )),
            handle: None,
        }
    }

    pub fn with_rtu_server(config: ServerConfig<T, ferrowl_modbus::rtu::Config>) -> Self {
        Self {
            builder: Builder::RtuServer(ferrowl_modbus::rtu::ServerBuilder::new(
                config.config,
                config.memory,
            )),
            handle: None,
        }
    }

    /// Spawns the endpoint's background task. Fails with
    /// [`InstanceError::AlreadyActive`] if it is still running.
    pub async fn start<L, S>(&mut self, log: L, status: S) -> Result<(), Error>
    where
        L: LogFn + Clone,
        S: LogFn + Clone,
    {
        if let Some(h) = &self.handle
            && !h.is_finished()
        {
            return Err(InstanceError::AlreadyActive.into());
        }

        match &self.builder {
            Builder::TcpClient(builder) => {
                let (sender, receiver) = tokio::sync::mpsc::channel(10);
                let res = builder.spawn(receiver, log, status).await;
                match res {
                    Err(e) => {
                        return Err(e.into());
                    }
                    Ok(handle) => {
                        self.handle = Some(Handle::Client(handle::ClientHandle { handle, sender }));
                    }
                }
            }
            Builder::TcpServer(builder) => {
                let res = builder.spawn(log).await;
                match res {
                    Err(e) => {
                        return Err(e.into());
                    }
                    Ok(handle) => {
                        self.handle = Some(Handle::Server(handle::ServerHandle { handle }));
                    }
                }
            }
            Builder::RtuClient(builder) => {
                let (sender, receiver) = tokio::sync::mpsc::channel(10);
                let res = builder.spawn(receiver, log, status).await;
                match res {
                    Err(e) => {
                        return Err(e.into());
                    }
                    Ok(handle) => {
                        self.handle = Some(Handle::Client(handle::ClientHandle { handle, sender }));
                    }
                }
            }
            Builder::RtuServer(builder) => {
                let res = builder.spawn(log).await;
                match res {
                    Err(e) => {
                        return Err(e.into());
                    }
                    Ok(handle) => {
                        self.handle = Some(Handle::Server(handle::ServerHandle { handle }));
                    }
                }
            }
        }
        Ok(())
    }

    /// Stops the running task: asks clients to terminate gracefully, then
    /// aborts the task if it is still alive.
    pub async fn stop(&mut self) -> Result<(), Error> {
        if self.handle.is_none() {
            return Err(InstanceError::NotRunning.into());
        }

        if self
            .send_command(ferrowl_modbus::Command::Terminate)
            .await
            .is_ok()
        {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        let handle = self.handle.take();

        let res = match handle {
            Some(Handle::Client(h)) => {
                if h.handle.is_finished() {
                    Ok(Ok(()))
                } else {
                    h.handle.abort();
                    h.handle.await
                }
            }
            Some(Handle::Server(h)) => {
                if h.handle.is_finished() {
                    Ok(Ok(()))
                } else {
                    h.handle.abort();
                    h.handle.await
                }
            }
            _ => {
                unreachable!("case is unreachable");
            }
        };

        match res {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => {
                if e.is_cancelled() {
                    Ok(())
                } else {
                    Err(InstanceError::CancelFailed.into())
                }
            }
        }
    }

    /// Forwards a write/terminate command to a running client. Errors if no
    /// task is running or the instance is a server.
    pub async fn send_command(&self, command: ferrowl_modbus::Command) -> Result<(), Error> {
        if self.handle.is_none() {
            return Err(InstanceError::NotRunning.into());
        }
        match &self.handle {
            Some(Handle::Client(handle)) => handle
                .sender
                .send(command)
                .await
                .map_err(|e| InstanceError::SendError(e).into()),
            _ => Err(InstanceError::InvalidOperation.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use ferrowl_modbus::{Command, FunctionCode, Key, Operation, SlaveKey, tcp};
    use ferrowl_store::Range;
    use parking_lot::RwLock as MemLock;
    use tokio::sync::RwLock;

    /// No-op log/status sink satisfying `LogFn + Clone`.
    fn sink() -> impl LogFn + Clone {
        |_s: String| async move {}
    }

    /// An OS-assigned free TCP port (bind to :0, read the port, drop the listener).
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// A `tcp::Config` pointed at a local port nothing is listening on. `start()` still
    /// succeeds (spawn itself never touches the network) — only the spawned task's
    /// internal reconnect loop sees the refused connection.
    fn dead_tcp_config() -> tcp::Config {
        tcp::Config {
            ip: "127.0.0.1".to_string(),
            port: free_port(),
            timeout_ms: 200,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        }
    }

    fn tcp_client_instance() -> Instance<SlaveKey> {
        let operations = Arc::new(RwLock::new(vec![Operation {
            slave_id: 1,
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 2),
        }]));
        Instance::with_tcp_client(config::ClientConfig {
            config: Arc::new(RwLock::new(dead_tcp_config())),
            operations,
            memory: Arc::new(MemLock::new(
                ferrowl_store::Memory::<Key<SlaveKey>>::default(),
            )),
        })
    }

    #[tokio::test]
    async fn start_twice_is_already_active() {
        let mut instance = tcp_client_instance();
        instance.start(sink(), sink()).await.expect("first start");
        assert!(instance.active());

        let err = instance.start(sink(), sink()).await.unwrap_err();
        assert!(matches!(err, Error::Instance(InstanceError::AlreadyActive)));

        instance.stop().await.expect("cleanup stop");
    }

    #[tokio::test]
    async fn stop_never_started_is_not_running() {
        let mut instance = tcp_client_instance();
        let err = instance.stop().await.unwrap_err();
        assert!(matches!(err, Error::Instance(InstanceError::NotRunning)));
    }

    #[tokio::test]
    /// MB-R-093 — sending a write command to an instance that is not running fails rather than being silently dropped.
    async fn send_command_never_started_is_not_running() {
        let instance = tcp_client_instance();
        let err = instance.send_command(Command::Terminate).await.unwrap_err();
        assert!(matches!(err, Error::Instance(InstanceError::NotRunning)));
    }

    /// `send_command` on a server-role handle must reject with `InvalidOperation`. A real
    /// server would need a bound TCP listener; instead we construct the `Handle::Server`
    /// variant directly (both are in-crate types), which exercises exactly the same
    /// branch in `send_command` without any real I/O.
    #[tokio::test]
    /// MB-R-093 — sending a write command to an instance whose role is a server fails with an error rather than being silently dropped.
    async fn send_command_on_server_is_invalid_operation() {
        let mut instance = tcp_client_instance();
        let task = tokio::spawn(async { Ok(()) });
        instance.handle = Some(handle::Handle::Server(handle::ServerHandle {
            handle: task,
        }));

        let err = instance.send_command(Command::Terminate).await.unwrap_err();
        assert!(matches!(
            err,
            Error::Instance(InstanceError::InvalidOperation)
        ));

        instance.stop().await.expect("cleanup stop");
    }

    #[tokio::test]
    /// MB-R-094 — stopping a client requests graceful termination and deactivates the instance.
    async fn graceful_stop_deactivates_instance() {
        let mut instance = tcp_client_instance();
        instance.start(sink(), sink()).await.expect("start");
        assert!(instance.active());

        instance.stop().await.expect("stop");
        assert!(!instance.active());
    }

    #[tokio::test]
    /// MB-R-094 — a stopped instance is restartable: after a graceful stop it can be started again.
    async fn stopped_instance_is_restartable() {
        let mut instance = tcp_client_instance();
        instance.start(sink(), sink()).await.expect("first start");
        assert!(instance.active());
        instance.stop().await.expect("stop");
        assert!(!instance.active());

        // Restart the same instance.
        instance.start(sink(), sink()).await.expect("restart");
        assert!(instance.active());
        instance.stop().await.expect("cleanup stop");
    }

    #[tokio::test]
    async fn active_reflects_finished_task() {
        let mut instance = tcp_client_instance();
        instance.start(sink(), sink()).await.expect("start");
        assert!(instance.active());

        // Force the client task to exit on its own by telling it to terminate, then give
        // it a moment to actually finish, without going through `stop()`'s bookkeeping.
        instance
            .send_command(Command::Terminate)
            .await
            .expect("send terminate");
        for _ in 0..50 {
            if !instance.active() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }
        assert!(!instance.active());

        // `stop()` on an already-finished task still tears down bookkeeping cleanly.
        instance.stop().await.expect("stop after natural finish");
    }
}
