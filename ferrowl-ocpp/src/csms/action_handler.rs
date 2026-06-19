//! The low-level, full-fidelity inbound handler for the CSMS role.

use std::future::Future;

use super::registry::ConnectionId;
use crate::action::Version;
use crate::error::CallError;

/// Answers CS-initiated Calls (e.g. `BootNotification`, `Heartbeat`, `StatusNotification`) with
/// full wire fidelity. Every method is scoped to the [`ConnectionId`] the Call arrived on so a
/// handler can distinguish concurrently connected charging stations. Returning
/// [`Err(CallError)`](CallError) rejects the Call at the protocol level without dropping the
/// connection.
pub trait CsmsActionHandler<V: Version>: Send + Sync + 'static {
    fn handle_call(
        &self,
        conn: ConnectionId,
        action: V::Action,
    ) -> impl Future<Output = Result<V::Response, CallError>> + Send;

    fn on_connected(&self, _conn: ConnectionId) -> impl Future<Output = ()> + Send {
        async {}
    }

    fn on_disconnected(&self, _conn: ConnectionId) -> impl Future<Output = ()> + Send {
        async {}
    }
}
