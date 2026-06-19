//! Server-level and per-connection commands for the CSMS role.

use tokio::sync::oneshot;

use super::registry::ConnectionId;
use crate::action::Version;
use crate::error::CallError;

/// A control message for a running CSMS server. Connection-scoped variants target one connected CS
/// by its [`ConnectionId`]; `Broadcast` fans out to every live connection.
pub enum Command<V: Version> {
    /// Stop the server: terminate every connection and the accept loop.
    Terminate,
    /// Send a Call to one connection without awaiting its reply.
    SendToConnection(ConnectionId, V::Action),
    /// Send a Call to one connection and receive its typed reply on the oneshot.
    SendToConnectionAwait(
        ConnectionId,
        V::Action,
        oneshot::Sender<Result<V::Response, CallError>>,
    ),
    /// Send a Call to every connected CS (fire-and-forget).
    Broadcast(V::Action),
    /// Disconnect a single connection.
    DisconnectConnection(ConnectionId),
}

/// Internal per-connection command, routed by the registry to a single connection's loop.
pub(crate) enum ConnCommand<V: Version> {
    Terminate,
    Fire(V::Action),
    Call(V::Action, oneshot::Sender<Result<V::Response, CallError>>),
}
