//! The low-level, full-fidelity inbound handler for the CS role.

use std::future::Future;

use crate::action::Version;
use crate::error::CallError;

/// Answers CSMS-initiated Calls (e.g. `RemoteStartTransaction`, `Reset`, `ChangeConfiguration`)
/// with full wire fidelity over `rust_ocpp`'s own request/response structs. Uses native RPITIT to
/// mirror [`LogFn`](crate::LogFn) — no `async-trait`. Returning [`Err(CallError)`](CallError)
/// rejects the Call at the protocol level (a `CallError` frame is sent back) without tearing down
/// the connection.
pub trait CsActionHandler<V: Version>: Send + Sync + 'static {
    fn handle_call(
        &self,
        action: V::Action,
    ) -> impl Future<Output = Result<V::Response, CallError>> + Send;

    fn on_connected(&self) -> impl Future<Output = ()> + Send {
        async {}
    }

    fn on_disconnected(&self) -> impl Future<Output = ()> + Send {
        async {}
    }
}
