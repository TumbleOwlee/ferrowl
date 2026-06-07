pub mod builder;
pub mod config;
pub mod error;
pub mod handle;

use builder::Builder;
use config::{ClientConfig, ServerConfig};
use error::{Error, InstanceError};
use handle::Handle;

use ferrowl_net::KeyParams;

pub struct Instance<T: KeyParams> {
    builder: Builder<T>,
    handle: Option<Handle>,
}

impl<T: KeyParams> Instance<T> {
    pub fn with_tcp_client(config: ClientConfig<T, ferrowl_net::tcp::Config>) -> Self {
        Self {
            builder: Builder::TcpClient(ferrowl_net::tcp::ClientBuilder::new(
                config.config,
                config.operations,
                config.memory,
            )),
            handle: None,
        }
    }

    pub fn with_rtu_client(config: ClientConfig<T, ferrowl_net::rtu::Config>) -> Self {
        Self {
            builder: Builder::RtuClient(ferrowl_net::rtu::ClientBuilder::new(
                config.config,
                config.operations,
                config.memory,
            )),
            handle: None,
        }
    }

    pub fn with_tcp_server(config: ServerConfig<T, ferrowl_net::tcp::Config>) -> Self {
        Self {
            builder: Builder::TcpServer(ferrowl_net::tcp::ServerBuilder::new(
                config.config,
                config.memory,
            )),
            handle: None,
        }
    }

    pub fn with_rtu_server(config: ServerConfig<T, ferrowl_net::rtu::Config>) -> Self {
        Self {
            builder: Builder::RtuServer(ferrowl_net::rtu::ServerBuilder::new(
                config.config,
                config.memory,
            )),
            handle: None,
        }
    }

    pub async fn start<L, S>(&mut self, log: L, status: S) -> Result<(), Error>
    where
        L: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
        S: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
        for<'a> L::CallRefFuture<'a>: Send,
        for<'a> S::CallRefFuture<'a>: Send,
    {
        if let Some(h) = &self.handle {
            if !h.is_finished() {
                return Err(InstanceError::AlreadyActive.into());
            }
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

    pub async fn stop(&mut self) -> Result<(), Error> {
        if self.handle.is_none() {
            return Err(InstanceError::NotRunning.into());
        }

        if let Ok(_) = self.send_command(ferrowl_net::Command::Terminate).await {
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

    pub async fn send_command(&self, command: ferrowl_net::Command) -> Result<(), Error> {
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
