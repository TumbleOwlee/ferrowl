//! Host-side action dispatch backing the `C_OCPP` Lua module. State access reuses the register
//! [`Read`](crate::module::Read)/[`Write`](crate::module::Write) traits; actions go through this.

use crate::module::ValueType;

/// Host hook for the version-specific `C_OCPP:<Action>(args)` methods. The set of action names is
/// a static property of the host (it differs between OCPP 1.6 and 2.0.1), so the Lua module can
/// register one method per name. `dispatch` enqueues the action; it returns `false` on error.
pub trait OcppActions {
    /// The action method names to expose as `C_OCPP:<Name>(args)` for this host's OCPP version.
    fn actions() -> Vec<&'static str>
    where
        Self: Sized;

    /// Enqueue `action` with flat key/value override args. Returns `false` if the action could not
    /// be enqueued (e.g. unknown action).
    fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool;
}
