//! Commands an external driver sends to a running [`Client`](crate::cs::Client) task.

use tokio::sync::oneshot;

use crate::action::Version;
use crate::error::CallError;

/// A control message for a running CS (charging-station/client) task. The OCPP analogue of
/// `ferrowl-modbus`'s `Command`: it lets a driver trigger actions on the live connection without
/// owning the websocket.
pub enum Command<V: Version> {
    /// Stop the client loop and tear the connection down.
    Terminate,
    /// Send a Call without awaiting its reply.
    SendAction(V::Action),
    /// Send a Call and receive its typed, decoded reply (or a peer rejection) on the oneshot.
    SendActionAwait(V::Action, oneshot::Sender<Result<V::Response, CallError>>),
}
