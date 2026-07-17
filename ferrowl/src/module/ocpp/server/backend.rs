//! Version-generic OCPP CSMS (server) backend: wraps the `ferrowl-ocpp` `Server<V>`, binds/unbinds
//! the listening socket, and funnels every connection lifecycle change and every inbound/outbound
//! OCPP message to the view through a single event channel. Unlike the client backend there is no
//! single shared message log — the view keeps a separate log per connected entry (CS / connector),
//! so all the backend does is deliver [`ServerEvent`]s the view sorts into the right entry.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use ferrowl_ocpp::csms::{Command, Config, ConnectionId, CsmsActionHandler, Server, ServerBuilder};
use ferrowl_ocpp::{Error, Version};

use crate::module::ocpp::client::backend::{Dir, OcppMessage};
use crate::module::ocpp::config::session::OcppSpec;
use crate::module::ocpp::lock::{with_state, with_state_mut};
pub use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::wire_log::encode_response_or_log;

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
        /// The entry scope this Call was sent from (CS-level/connector/EVSE), for log routing.
        scope: Scope,
        name: String,
        request: Value,
        response: Value,
        ok: bool,
        context: String,
    },
}

pub type EventTx = mpsc::UnboundedSender<ServerEvent>;
pub type EventRx = mpsc::UnboundedReceiver<ServerEvent>;

/// CSMS RFID accept-lists, split by level: a charge-point-wide (CS) list plus per-connector/EVSE
/// lists keyed by [`Scope`]. A connector inherits the CS list (its effective set is the connector
/// list unioned with the CS list). An *empty effective set accepts every tag* (open mode); once any
/// tag is listed in the effective set, only listed tags pass.
#[derive(Debug, Clone, Default)]
pub struct RfidStore {
    /// Charge-point-wide tags, inherited by every connector.
    pub cs: Vec<String>,
    /// Per-connector/EVSE tags, keyed by the connector's [`Scope`].
    pub by_scope: HashMap<Scope, Vec<String>>,
}

impl RfidStore {
    /// The connector list for `scope` (empty if none recorded).
    pub fn scope_list(&self, scope: Scope) -> &[String] {
        self.by_scope.get(&scope).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Add `tag` to a level (deduplicated); returns whether it was newly inserted. `scope`
    /// [`Scope::CS`] targets the charge-point-wide list, otherwise the connector list.
    pub fn add(&mut self, scope: Scope, tag: String) -> bool {
        let list = if scope == Scope::CS {
            &mut self.cs
        } else {
            self.by_scope.entry(scope).or_default()
        };
        if list.contains(&tag) {
            false
        } else {
            list.push(tag);
            true
        }
    }

    /// Remove `tag` from a level; returns whether it was present.
    pub fn remove(&mut self, scope: Scope, tag: &str) -> bool {
        let list = if scope == Scope::CS {
            Some(&mut self.cs)
        } else {
            self.by_scope.get_mut(&scope)
        };
        match list {
            Some(list) => {
                let before = list.len();
                list.retain(|t| t != tag);
                list.len() < before
            }
            None => false,
        }
    }
}

/// Shared CSMS RFID accept-lists, edited live by the view (detail dialogs / `:rfid`) and read by the
/// inbound handler to gate Authorize / transaction starts.
pub type RfidLists = Arc<RwLock<RfidStore>>;

/// Run `f` with a read guard on `store`, dropped before returning.
pub fn with_rfids<R>(store: &RfidLists, f: impl FnOnce(&RfidStore) -> R) -> R {
    with_state(store, f)
}

/// Run `f` with a write guard on `store`, dropped before returning.
pub fn with_rfids_mut<R>(store: &RfidLists, f: impl FnOnce(&mut RfidStore) -> R) -> R {
    with_state_mut(store, f)
}

/// Whether a tag passes a CS-wide check (Authorize, which carries no connector): accepted if the
/// effective set — the CS list unioned with *every* connector list — is empty or contains the tag.
pub fn cs_authorized(store: &RfidLists, id_tag: &str) -> bool {
    with_rfids(store, |s| {
        let mut empty = s.cs.is_empty();
        if s.cs.iter().any(|t| t == id_tag) {
            return true;
        }
        for list in s.by_scope.values() {
            if !list.is_empty() {
                empty = false;
                if list.iter().any(|t| t == id_tag) {
                    return true;
                }
            }
        }
        empty
    })
}

/// Whether a tag passes a connector-scoped check (a transaction start that names a connector):
/// accepted if the effective set — that connector's list unioned with the inherited CS list — is
/// empty or contains the tag.
pub fn scope_authorized(store: &RfidLists, scope: Scope, id_tag: &str) -> bool {
    with_rfids(store, |s| {
        let conn = s.scope_list(scope);
        let effective_empty = s.cs.is_empty() && conn.is_empty();
        effective_empty || s.cs.iter().chain(conn).any(|t| t == id_tag)
    })
}

/// Which TLS mode a [`OcppServer::start`] actually bound with — returned so the caller's log
/// line reports the listener's real state instead of re-deriving (and possibly mispredicting)
/// it from the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsBinding {
    Plain,
    SelfSigned,
    Certificates,
}

/// The version-generic CSMS backend owned by a server view.
///
/// Deliberately holds no copy of the module spec: the listener config is built from the spec the
/// view passes into each [`start`](Self::start) call, so an edited endpoint/security section can
/// never drift from what the listener actually binds with.
pub struct OcppServer<V: Version> {
    server: Option<Server<V>>,
    /// Server bound state (drives the ONLINE/OFFLINE status line).
    online: Arc<AtomicBool>,
}

impl<V: Version> OcppServer<V>
where
    V::Action: Clone,
{
    pub fn new() -> Self {
        Self {
            server: None,
            online: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    /// Bind the listening socket and spawn the accept loop with the caller-supplied inbound handler.
    /// Idempotent: a no-op if already bound.
    pub async fn start<H: CsmsActionHandler<V>>(
        &mut self,
        spec: &OcppSpec,
        handler: H,
    ) -> Result<TlsBinding, Error> {
        // A wss endpoint without configured TLS material falls back to an ephemeral
        // self-signed certificate instead of silently binding plain TCP.
        let tls = spec.effective_csms_tls();
        let binding = match &tls {
            None => TlsBinding::Plain,
            Some(cfg) => match cfg.mode {
                ferrowl_ocpp::CsmsTlsMode::SelfSigned => TlsBinding::SelfSigned,
                ferrowl_ocpp::CsmsTlsMode::Files { .. } => TlsBinding::Certificates,
            },
        };
        if self.server.is_some() {
            return Ok(binding);
        }
        let config = Config {
            host: spec.ip.clone(),
            port: spec.port,
            timeout_ms: spec.timeout_ms.unwrap_or(30_000),
            basic_auth: spec.security.basic_auth(),
            tls,
        };
        let server = ServerBuilder::<V>::new(config)
            .spawn(handler, |_s: String| async {})
            .await?;
        self.server = Some(server);
        self.online.store(true, Ordering::Relaxed);
        Ok(binding)
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
            Ok(Ok(response)) => Ok(encode_response_or_log::<V>(&response)),
            Ok(Err(call_err)) => Err(Error::Call(call_err)),
            Err(_) => Err(Error::ChannelClosed),
        }
    }
}

/// Build a message-log entry for a request (inbound) / its reply (outbound).
pub fn inbound_messages(name: &str, request: Value, response: Value) -> [OcppMessage; 2] {
    [
        OcppMessage::new(Dir::In, name, request, None, String::new()),
        OcppMessage::new(Dir::Out, name, response, Some(true), String::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(cs: &[&str]) -> RfidLists {
        Arc::new(RwLock::new(RfidStore {
            cs: cs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }))
    }

    #[test]
    /// OC-R-074 — the CSMS maintains charge-point-wide and per-connector RFID accept-lists, deduplicating entries.
    fn ut_add_remove_dedup() {
        let mut s = RfidStore::default();
        assert!(s.add(Scope::CS, "A".into()));
        assert!(!s.add(Scope::CS, "A".into())); // duplicate
        assert!(s.add(Scope::connector(1), "B".into()));
        assert_eq!(s.scope_list(Scope::connector(1)), ["B"]);
        assert!(s.remove(Scope::connector(1), "B"));
        assert!(!s.remove(Scope::connector(1), "B")); // already gone
        assert!(s.scope_list(Scope::connector(1)).is_empty());
    }

    #[test]
    /// OC-R-075 — an empty effective accept-set (nothing listed anywhere) accepts every tag.
    fn ut_empty_everywhere_accepts_all() {
        let s = store(&[]);
        assert!(cs_authorized(&s, "ANY"));
        assert!(scope_authorized(&s, Scope::connector(1), "ANY"));
    }

    #[test]
    /// OC-R-076 — a charge-point-wide authorization is checked against the cp-wide list unioned with every connector list.
    fn ut_cs_authorize_unions_all_connectors() {
        let s = store(&["CS"]);
        with_rfids_mut(&s, |s| s.add(Scope::connector(2), "CONN2".into()));
        // The CS list and any connector list both authorize at the CS (connector-less) level.
        assert!(cs_authorized(&s, "CS"));
        assert!(cs_authorized(&s, "CONN2"));
        // A tag listed nowhere is rejected (the effective set is non-empty).
        assert!(!cs_authorized(&s, "NOPE"));
    }

    #[test]
    /// OC-R-074 — a connector's effective accept-set is its own list unioned with the charge-point-wide list.
    fn ut_scope_authorize_inherits_cs_only() {
        let s = store(&["CS"]);
        with_rfids_mut(&s, |s| s.add(Scope::connector(1), "CONN1".into()));
        // Connector 1 accepts its own tag and the inherited CS tag.
        assert!(scope_authorized(&s, Scope::connector(1), "CONN1"));
        assert!(scope_authorized(&s, Scope::connector(1), "CS"));
        // Another connector's tag is NOT inherited sideways.
        assert!(!scope_authorized(&s, Scope::connector(2), "CONN1"));
        // Connector 2 still inherits CS (its own list is empty, CS is not).
        assert!(scope_authorized(&s, Scope::connector(2), "CS"));
        assert!(!scope_authorized(&s, Scope::connector(2), "NOPE"));
    }
}
