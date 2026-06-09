// Crate
use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::server_core::handle_request;
use crate::{Error, Key, KeyParams, LogFn, SerialError};

// Workspace
use ferrowl_mem::Memory;

// External
use std::future;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_modbus::prelude::{ExceptionCode, Response, SlaveRequest};
use tokio_modbus::server::rtu::Server as RtuServer;
use tokio_serial::SerialStream;

/// Builds and spawns a Modbus RTU server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<RwLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<RwLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    /// Opens the configured serial port and spawns the serve loop as a
    /// tokio task. `log` receives log lines.
    pub async fn spawn<L>(&self, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
    {
        let guard = self.config.read().await;
        Server::run(&guard, self.memory.clone(), log).await
    }
}

/// Modbus RTU server: a [`tokio_modbus::server::Service`] that answers
/// each request directly from the shared memory.
pub struct Server<T, L>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    memory: Arc<RwLock<Memory<Key<T>>>>,
    log: L,
}

impl<T, L> Server<T, L>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    fn new(memory: Arc<RwLock<Memory<Key<T>>>>, log: L) -> Self {
        Self { memory, log }
    }

    async fn run(
        config: &Config,
        memory: Arc<RwLock<Memory<Key<T>>>>,
        log: L,
    ) -> Result<JoinHandle<Result<(), Error>>, Error> {
        let builder = serial_config_from(
            &config.path,
            config.baud_rate,
            config.data_bits,
            config.stop_bits,
            config.parity.as_deref(),
        )?;
        match SerialStream::open(&builder) {
            Ok(serial_stream) => {
                let rtu_server = RtuServer::new(serial_stream);
                let server = Server::new(memory, log);
                Ok(tokio::task::spawn(async {
                    rtu_server
                        .serve_forever(server)
                        .await
                        .map_err(Error::Server)
                }))
            }
            Err(e) => Err(SerialError::Error(e).into()),
        }
    }
}

impl<T, L> tokio_modbus::server::Service for Server<T, L>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    type Request = SlaveRequest<'static>;
    type Exception = ExceptionCode;
    type Response = Response;
    type Future = future::Ready<Result<Response, ExceptionCode>>;

    fn call(&self, request: Self::Request) -> Self::Future {
        let SlaveRequest { slave, request } = request;
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                handle_request(slave, request, &self.memory, &self.log, false).await
            })
        });
        future::ready(response)
    }
}
