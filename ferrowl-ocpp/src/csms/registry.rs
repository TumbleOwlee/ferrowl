//! Tracks the live CS connections accepted by a CSMS server.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;

use super::command::ConnCommand;
use crate::action::Version;

/// Opaque, server-assigned connection key (Decision 3). The charge-point identity parsed from the
/// URL path is kept as metadata, not used as the key, so reconnects and duplicate identities are
/// handled cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn#{}", self.0)
    }
}

/// Per-connection routing entry: the channel into the connection's command loop plus its identity.
struct ConnectionHandle<V: Version> {
    cmd_tx: mpsc::Sender<ConnCommand<V>>,
    identity: Option<String>,
}

/// Routes CSMS-level commands to the right per-connection task and assigns connection ids.
///
/// Exposed (via [`Server::registry`](crate::csms::Server::registry)) for read-only queries:
/// [`ConnectionRegistry::connection_ids`] and [`ConnectionRegistry::identity`].
pub struct ConnectionRegistry<V: Version> {
    next_id: AtomicU64,
    connections: RwLock<HashMap<ConnectionId, ConnectionHandle<V>>>,
}

impl<V: Version> ConnectionRegistry<V> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            connections: RwLock::new(HashMap::new()),
        })
    }

    /// Reserve a fresh connection id.
    pub(crate) fn next_id(&self) -> ConnectionId {
        ConnectionId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn insert(
        &self,
        id: ConnectionId,
        cmd_tx: mpsc::Sender<ConnCommand<V>>,
        identity: Option<String>,
    ) {
        self.connections
            .write()
            .unwrap()
            .insert(id, ConnectionHandle { cmd_tx, identity });
    }

    pub(crate) fn remove(&self, id: ConnectionId) {
        self.connections.write().unwrap().remove(&id);
    }

    pub(crate) fn sender(&self, id: ConnectionId) -> Option<mpsc::Sender<ConnCommand<V>>> {
        self.connections
            .read()
            .unwrap()
            .get(&id)
            .map(|h| h.cmd_tx.clone())
    }

    /// Senders for every live connection (for broadcast).
    pub(crate) fn all_senders(&self) -> Vec<mpsc::Sender<ConnCommand<V>>> {
        self.connections
            .read()
            .unwrap()
            .values()
            .map(|h| h.cmd_tx.clone())
            .collect()
    }

    /// Snapshot of currently connected ids.
    pub fn connection_ids(&self) -> Vec<ConnectionId> {
        self.connections.read().unwrap().keys().copied().collect()
    }

    /// The charge-point identity for a connection, if one was provided at handshake.
    pub fn identity(&self, id: ConnectionId) -> Option<String> {
        self.connections
            .read()
            .unwrap()
            .get(&id)
            .and_then(|h| h.identity.clone())
    }
}
