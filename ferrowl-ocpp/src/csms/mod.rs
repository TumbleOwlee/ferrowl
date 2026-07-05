//! CSMS = Charging Station Management System (server role; accepts CS connections).

mod action_handler;
mod command;
mod config;
mod core;
mod registry;
mod tls_stream;

use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};

use self::command::ConnCommand;
use self::tls_stream::ServerStream;
use crate::action::Version;
use crate::error::{CallError, Error};
use crate::log::LogFn;
use crate::ocppj::CallErrorCode;
use crate::security::BasicAuth;

pub use action_handler::CsmsActionHandler;
pub use command::Command;
pub use config::Config;
pub use registry::{ConnectionId, ConnectionRegistry};

/// Capacity of the server command channel and each per-connection channel.
const COMMAND_CHANNEL_CAP: usize = 32;

/// Builds and binds a CSMS server for a specific OCPP [`Version`].
pub struct ServerBuilder<V: Version> {
    config: Config,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> ServerBuilder<V>
where
    V::Action: Clone,
{
    pub fn new(config: Config) -> Self {
        Self {
            config,
            _v: PhantomData,
        }
    }

    /// Bind the listening socket and spawn the accept loop. `handler` answers inbound Calls for
    /// every connection.
    pub async fn spawn<H, L>(self, handler: H, log: L) -> Result<Server<V>, Error>
    where
        H: CsmsActionHandler<V>,
        L: LogFn + Clone,
    {
        let listener = TcpListener::bind((self.config.host.as_str(), self.config.port)).await?;
        let local_addr = listener.local_addr()?;

        let tls = self
            .config
            .tls
            .as_ref()
            .map(|tls| tls.build_server_config())
            .transpose()?;

        let handler = Arc::new(handler);
        let registry = ConnectionRegistry::<V>::new();
        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAP);

        let handle = tokio::spawn(accept_loop::<V, H, L>(
            listener,
            handler,
            registry.clone(),
            cmd_rx,
            log,
            self.config.timeout(),
            self.config.basic_auth.clone(),
            tls,
        ));

        Ok(Server {
            cmd_tx,
            registry,
            local_addr,
            handle: Some(handle),
            _v: PhantomData,
        })
    }
}

/// A handle to a running CSMS server task.
pub struct Server<V: Version> {
    cmd_tx: mpsc::Sender<Command<V>>,
    registry: Arc<ConnectionRegistry<V>>,
    local_addr: SocketAddr,
    handle: Option<JoinHandle<()>>,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> Server<V> {
    /// The bound local address (useful when the configured port was `0`).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Access the connection registry to enumerate connections or look up identities.
    pub fn registry(&self) -> &Arc<ConnectionRegistry<V>> {
        &self.registry
    }

    /// Clone of the server command sender.
    pub fn sender(&self) -> mpsc::Sender<Command<V>> {
        self.cmd_tx.clone()
    }

    /// Send a raw command to the server task.
    pub async fn send(&self, command: Command<V>) -> Result<(), Error> {
        self.cmd_tx
            .send(command)
            .await
            .map_err(|_| Error::ChannelClosed)
    }

    /// Send a Call to one connection and await its typed reply.
    pub async fn call(&self, conn: ConnectionId, action: V::Action) -> Result<V::Response, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(Command::SendToConnectionAwait(conn, action, reply_tx))
            .await?;
        match reply_rx.await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(call_err)) => Err(Error::Call(call_err)),
            Err(_) => Err(Error::ChannelClosed),
        }
    }

    /// Terminate the server (and all connections) and wait for it to finish.
    pub async fn terminate(mut self) -> Result<(), Error> {
        let _ = self.cmd_tx.send(Command::Terminate).await;
        self.join().await
    }

    /// Wait for the server task to finish.
    pub async fn join(&mut self) -> Result<(), Error> {
        if let Some(handle) = self.handle.take() {
            handle.await.map_err(|_| Error::NotRunning)?;
        }
        Ok(())
    }
}

/// Derive the OCPP-J charge-point identity from the request URL path's last non-empty segment.
fn identity_from_path(path: &str) -> Option<String> {
    path.rsplit('/')
        .find(|seg| !seg.is_empty())
        .map(str::to_owned)
}

/// The accept loop: hand-shakes new sockets and routes server-level commands.
#[allow(clippy::too_many_arguments)]
async fn accept_loop<V, H, L>(
    listener: TcpListener,
    handler: Arc<H>,
    registry: Arc<ConnectionRegistry<V>>,
    mut commands: mpsc::Receiver<Command<V>>,
    log: L,
    timeout: std::time::Duration,
    basic_auth: Option<BasicAuth>,
    tls: Option<Arc<rustls::ServerConfig>>,
) where
    V: Version,
    V::Action: Clone,
    H: CsmsActionHandler<V>,
    L: LogFn + Clone,
{
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let handler = handler.clone();
                    let registry = registry.clone();
                    let log = log.clone();
                    let basic_auth = basic_auth.clone();
                    let tls = tls.clone();
                    tokio::spawn(async move {
                        let stream = match tls {
                            Some(tls_config) => {
                                let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => ServerStream::Tls(Box::new(tls_stream)),
                                    Err(e) => {
                                        log.invoke(format!("CSMS TLS handshake failed from {peer}: {e}")).await;
                                        return;
                                    }
                                }
                            }
                            None => ServerStream::Plain(stream),
                        };
                        let identity_cell = Arc::new(Mutex::new(None));
                        let cell = identity_cell.clone();
                        let callback = move |req: &Request, mut resp: Response| {
                            *cell.lock().unwrap() = identity_from_path(req.uri().path());
                            if let Some(auth) = &basic_auth
                                && !auth.matches(req.headers().get("authorization"))
                            {
                                return Err(reject_unauthorized());
                            }
                            if !subprotocol_matches(req, V::subprotocol()) {
                                return Err(reject_subprotocol());
                            }
                            resp.headers_mut().append(
                                "Sec-WebSocket-Protocol",
                                HeaderValue::from_static(V::subprotocol()),
                            );
                            Ok(resp)
                        };
                        let ws = match accept_hdr_async(stream, callback).await {
                            Ok(ws) => ws,
                            Err(e) => {
                                log.invoke(format!("CSMS handshake failed from {peer}: {e}")).await;
                                return;
                            }
                        };
                        let conn = registry.next_id();
                        let identity = identity_cell.lock().unwrap().clone();
                        let (conn_tx, conn_rx) = mpsc::channel(COMMAND_CHANNEL_CAP);
                        registry.insert(conn, conn_tx, identity);
                        core::run_connection::<V, H, _, _>(
                            ws, handler, conn, conn_rx, registry.clone(), log, timeout,
                        )
                        .await;
                    });
                }
                Err(e) => log.invoke(format!("CSMS accept error: {e}")).await,
            },
            cmd = commands.recv() => match cmd {
                None | Some(Command::Terminate) => {
                    for tx in registry.all_senders() {
                        let _ = tx.send(ConnCommand::Terminate).await;
                    }
                    break;
                }
                Some(Command::SendToConnection(id, action)) => match registry.sender(id) {
                    Some(tx) => { let _ = tx.send(ConnCommand::Fire(action)).await; }
                    None => log.invoke(format!("CSMS: no such connection {id}")).await,
                },
                Some(Command::SendToConnectionAwait(id, action, reply_tx)) => match registry.sender(id) {
                    Some(tx) => { let _ = tx.send(ConnCommand::Call(action, reply_tx)).await; }
                    None => {
                        let _ = reply_tx.send(Err(CallError::new(
                            CallErrorCode::InternalError,
                            format!("no such connection {id}"),
                        )));
                    }
                },
                Some(Command::Broadcast(action)) => {
                    for tx in registry.all_senders() {
                        let _ = tx.send(ConnCommand::Fire(action.clone())).await;
                    }
                }
                Some(Command::DisconnectConnection(id)) => {
                    if let Some(tx) = registry.sender(id) {
                        let _ = tx.send(ConnCommand::Terminate).await;
                    }
                }
            },
        }
    }
}

/// Whether the handshake request advertises the expected subprotocol token.
fn subprotocol_matches(req: &Request, expected: &str) -> bool {
    req.headers()
        .get_all("sec-websocket-protocol")
        .iter()
        .any(|value| {
            value
                .to_str()
                .map(|s| s.split(',').any(|t| t.trim() == expected))
                .unwrap_or(false)
        })
}

/// Build the 400 response used to reject a mismatched subprotocol.
fn reject_subprotocol() -> ErrorResponse {
    let mut resp = ErrorResponse::new(Some("unsupported OCPP subprotocol".to_owned()));
    *resp.status_mut() = StatusCode::BAD_REQUEST;
    resp
}

/// Build the 401 response used to reject a missing or mismatched Basic Auth credential
/// (Security Profile 1). Never includes the expected credential in the response body.
fn reject_unauthorized() -> ErrorResponse {
    let mut resp = ErrorResponse::new(Some("authentication required".to_owned()));
    *resp.status_mut() = StatusCode::UNAUTHORIZED;
    resp
}
