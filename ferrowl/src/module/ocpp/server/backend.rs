//! Version-generic OCPP CSMS (server) backend: wraps the `ferrowl-ocpp` `Server<V>`, binds/unbinds
//! the listening socket, and funnels every connection lifecycle change and every inbound/outbound
//! OCPP message to the view through a single event channel. Unlike the client backend there is no
//! single shared message log — the view keeps a separate log per connected entry (CS / connector),
//! so all the backend does is deliver [`ServerEvent`]s the view sorts into the right entry.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use ferrowl_ocpp::csms::{Command, Config, ConnectionId, CsmsActionHandler, Server, ServerBuilder};
use ferrowl_ocpp::{Error, Version};

use crate::module::ocpp::client::backend::{Dir, OcppMessage, now_ms};
use crate::module::ocpp::config::session::OcppSpec;

/// A lifecycle/message event delivered from the CSMS server tasks to the view. Version-agnostic:
/// action payloads are carried as JSON so the (version-specific) view extracts connector ids and
/// state from them.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// A charging station completed its WebSocket handshake. The identity is resolved by the view
    /// from the registry (see [`OcppServer::identity`]).
    Connected { conn: ConnectionId },
    /// A charging station's connection ended.
    Disconnected { conn: ConnectionId },
    /// An inbound CS→CSMS Call and the CSMS's reply to it.
    Inbound {
        conn: ConnectionId,
        name: String,
        request: Value,
        response: Value,
    },
    /// An outbound CSMS→CS Call (initiated by the view) and the CS's reply, or an error string.
    Outbound {
        conn: ConnectionId,
        /// The connector entry this Call was sent from (`None` = CS-level entry), for log routing.
        connector_id: Option<i64>,
        name: String,
        request: Value,
        response: Value,
        ok: bool,
        context: String,
    },
}

pub type EventTx = mpsc::UnboundedSender<ServerEvent>;
pub type EventRx = mpsc::UnboundedReceiver<ServerEvent>;

/// The version-generic CSMS backend owned by a server view.
pub struct OcppServer<V: Version> {
    spec: OcppSpec,
    server: Option<Server<V>>,
    /// Server bound state (drives the ONLINE/OFFLINE status line).
    online: Arc<AtomicBool>,
}

impl<V: Version> OcppServer<V>
where
    V::Action: Clone,
{
    pub fn new(spec: OcppSpec) -> Self {
        Self {
            spec,
            server: None,
            online: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    /// Bind the listening socket and spawn the accept loop with the caller-supplied inbound handler.
    /// Idempotent: a no-op if already bound.
    pub async fn start<H: CsmsActionHandler<V>>(&mut self, handler: H) -> Result<(), Error> {
        if self.server.is_some() {
            return Ok(());
        }
        let config = Config {
            host: self.spec.ip.clone(),
            port: self.spec.port,
            timeout_ms: self.spec.timeout_ms.unwrap_or(30_000),
        };
        let server = ServerBuilder::<V>::new(config)
            .spawn(handler, |_s: String| async {})
            .await?;
        self.server = Some(server);
        self.online.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Terminate the server task and every connection, if running.
    pub async fn stop(&mut self) -> Result<(), Error> {
        self.online.store(false, Ordering::Relaxed);
        match self.server.take() {
            Some(s) => s.terminate().await,
            None => Ok(()),
        }
    }

    /// The bound local address (`host:port`) when running, for the status line.
    pub fn bound_addr(&self) -> Option<String> {
        self.server.as_ref().map(|s| s.local_addr().to_string())
    }

    /// The charge-point identity for a connection (URL-path segment), if known.
    pub fn identity(&self, conn: ConnectionId) -> Option<String> {
        self.server
            .as_ref()
            .and_then(|s| s.registry().identity(conn))
    }

    /// A detachable sender for off-thread Calls to a specific connection, decoupled from the
    /// `OcppServer` borrow so the round-trip can be `tokio::spawn`ed. `None` when not bound.
    pub fn sender(&self) -> Option<OcppServerSender<V>> {
        self.server
            .as_ref()
            .map(|s| OcppServerSender { cmd_tx: s.sender() })
    }
}

/// A self-contained Call sender to one connection, decoupled from the [`OcppServer`] borrow.
pub struct OcppServerSender<V: Version> {
    cmd_tx: mpsc::Sender<Command<V>>,
}

impl<V: Version> OcppServerSender<V> {
    /// Send a typed Call to `conn` and await its reply. Mirrors `Server::call` but over a cloned
    /// command channel so it can run in a spawned task.
    pub async fn call(self, conn: ConnectionId, action: V::Action) -> Result<Value, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::SendToConnectionAwait(conn, action, reply_tx))
            .await
            .map_err(|_| Error::ChannelClosed)?;
        match reply_rx.await {
            Ok(Ok(response)) => Ok(V::encode_response(&response).unwrap_or(Value::Null)),
            Ok(Err(call_err)) => Err(Error::Call(call_err)),
            Err(_) => Err(Error::ChannelClosed),
        }
    }
}

/// Build a message-log entry for a request (inbound) / its reply (outbound).
pub fn inbound_messages(name: &str, request: Value, response: Value) -> [OcppMessage; 2] {
    [
        OcppMessage {
            ts: now_ms(),
            direction: Dir::In,
            name: name.to_string(),
            payload: request,
            ok: None,
            context: String::new(),
        },
        OcppMessage {
            ts: now_ms(),
            direction: Dir::Out,
            name: name.to_string(),
            payload: response,
            ok: Some(true),
            context: String::new(),
        },
    ]
}
