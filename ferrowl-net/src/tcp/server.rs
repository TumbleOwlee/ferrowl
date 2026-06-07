// Crate
use crate::common::handle_request;
use crate::tcp::Config;
use crate::{Error, Key, KeyParams, TcpError};

// Workspace
use ferrowl_mem::Memory;

// External
use std::future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_modbus::prelude::{ExceptionCode, Response, SlaveRequest};
use tokio_modbus::server::tcp::{Server as TcpServer, accept_tcp_connection};

pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<RwLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<RwLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    pub async fn spawn<L>(&self, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
        for<'a> L::CallRefFuture<'a>: Send,
    {
        let guard = self.config.read().await;
        Server::run(&guard, self.memory.clone(), log).await
    }
}

pub struct Server<T, L>
where
    T: KeyParams,
    L: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
    for<'a> L::CallRefFuture<'a>: Send,
{
    memory: Arc<RwLock<Memory<Key<T>>>>,
    log: L,
}

impl<T, L> Server<T, L>
where
    T: KeyParams,
    L: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
    for<'a> L::CallRefFuture<'a>: Send,
{
    fn new(memory: Arc<RwLock<Memory<Key<T>>>>, log: L) -> Self {
        Self { memory, log }
    }

    async fn run(
        config: &Config,
        memory: Arc<RwLock<Memory<Key<T>>>>,
        log: L,
    ) -> Result<JoinHandle<Result<(), Error>>, Error> {
        let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
            .parse()
            .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let server = TcpServer::new(listener);
                let memory = memory.clone();
                let log = log.clone();
                Ok(tokio::task::spawn(async move {
                    let new_request_handler = |_socket_addr| {
                        Ok(Some(Server::new(memory.clone(), log.clone())))
                    };
                    let on_connected = |stream, socket_addr| async move {
                        accept_tcp_connection(stream, socket_addr, new_request_handler)
                    };
                    let on_process_log = log.clone();
                    let on_process_error = move |err| {
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                on_process_log(format!("Server processing failed. [{}]", err))
                                    .await;
                            })
                        })
                    };
                    server
                        .serve(&on_connected, on_process_error)
                        .await
                        .map_err(Error::Server)
                }))
            }
            Err(e) => Err(Error::Server(e)),
        }
    }
}

impl<T, L> tokio_modbus::server::Service for Server<T, L>
where
    T: KeyParams,
    L: AsyncFn(String) -> () + Clone + Send + Sync + 'static,
    for<'a> L::CallRefFuture<'a>: Send,
{
    type Request = SlaveRequest<'static>;
    type Exception = ExceptionCode;
    type Response = Response;
    type Future = future::Ready<Result<Response, ExceptionCode>>;

    fn call(&self, request: Self::Request) -> Self::Future {
        let SlaveRequest { slave, request } = request;
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                handle_request(slave, request, &self.memory, &self.log, true).await
            })
        });
        future::ready(response)
    }
}
