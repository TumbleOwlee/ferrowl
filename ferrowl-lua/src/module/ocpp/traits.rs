//! Host-side action dispatch backing the `C_OCPP` Lua module. State access reuses the register
//! [`Read`](crate::module::Read)/[`Write`](crate::module::Write) traits; actions go through this.

use crate::module::{Read, ValueType, Write};

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

/// A handle providing the full `Get`/`Set`/`<Action>` surface for one OCPP scope (a charging
/// station or a single connector), as wrapped by an [`Accessor`](super::Accessor).
pub trait OcppHandle: Read + Write + OcppActions + 'static {}
impl<T: Read + Write + OcppActions + 'static> OcppHandle for T {}

/// Host backing the **client** `C_OCPP` module: bare `Get`/`Set`/`<Action>` address the
/// charging-station-level state (via [`Read`]/[`Write`]/[`OcppActions`]); `Connector(id)` resolves
/// a per-connector accessor handle.
pub trait OcppClientHost {
    /// The accessor handle for one connector.
    type Conn: OcppHandle;

    /// Resolve the accessor handle for connector `id`. The handle reads/writes that connector's
    /// state and dispatches connector-scoped actions; an unknown id yields a handle whose reads
    /// surface as errors (returned to Lua as such).
    fn connector(&self, id: i64) -> Self::Conn;

    /// Connector ids known for the client station
    fn connectors(&self) -> Vec<i64>;
}

/// Host backing the **server** `C_OCPP` module: one Lua sim spans every connected charging
/// station, so access is keyed by station identity. `GetChargingStations`/`GetConnectors`
/// enumerate; `ChargingStation(cs)`/`Connector(cs, id)` resolve scope accessors.
pub trait OcppServerHost {
    /// The CS-level accessor handle for a station.
    type Station: OcppHandle;
    /// The per-connector accessor handle for a station.
    type Conn: OcppHandle;

    /// Identities of the currently connected charging stations.
    fn stations(&self) -> Vec<String>;
    /// Connector ids known for station `cs` (empty if the station is unknown).
    fn connectors(&self, cs: &str) -> Vec<i64>;
    /// Resolve the CS-level accessor handle for station `cs` (`None` if unknown).
    fn station(&self, cs: &str) -> Option<Self::Station>;
    /// Resolve the connector accessor handle for `(cs, id)` (`None` if unknown).
    fn connector(&self, cs: &str, id: i64) -> Option<Self::Conn>;
}
