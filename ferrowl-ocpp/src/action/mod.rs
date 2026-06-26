//! The low-level, full-fidelity action layer.
//!
//! Each OCPP version is described by a [`Version`] implementation that ties together its generated
//! `Action`/`Response` enums (one variant per action, wrapping `rust_ocpp`'s own request/response
//! structs untouched) with the codec glue the core loop needs. Implementations are produced by the
//! [`define_ocpp_version!`](crate::action::macros::define_ocpp_version) macro.

pub(crate) mod macros;

#[cfg(feature = "v1_6")]
pub mod v1_6;
#[cfg(feature = "v2_0_1")]
pub mod v2_0_1;
#[cfg(feature = "v2_1")]
pub mod v2_1;

use serde_json::Value;

use crate::error::{OcppError, ValidationError};

/// Whether a CSMS-originated action's request carries a connector/EVSE target, and if so whether
/// that target is mandatory. Drives the server UI's split between CS-level and connector actions:
/// `None` → CS-level only, `Required` → connector entries only, `Optional` → shown on both (the
/// CS-level entry omits the connector id; a connector entry injects its own). For OCPP 2.0.1 the
/// target is the `evse`/`evseId` field, treated the same as a 1.6 `connectorId` for UI purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorScope {
    /// No connector/EVSE field — a charge-point-wide action.
    None,
    /// Optional connector/EVSE field — usable both CS-wide and per connector.
    Optional,
    /// Mandatory connector/EVSE field — only meaningful for a specific connector.
    Required,
}

/// Everything the version-agnostic core loop needs to move a single OCPP version's actions on and
/// off the wire. Both inbound directions (decode a peer Call, decode a peer's CallResult) and both
/// outbound directions (encode our Call, encode our response) are covered.
///
/// A `CallResult` frame carries no action name, so [`Version::decode_result`] takes the originating
/// `Action` to know which response variant to build.
pub trait Version: Send + Sync + 'static {
    /// The generated per-version action enum (one variant per action).
    type Action: Send + 'static;
    /// The generated per-version response enum (one variant per action).
    type Response: Send + 'static;

    /// The wire action name for an action variant (e.g. `"BootNotification"`).
    fn action_name(action: &Self::Action) -> &'static str;

    /// Every action's wire name for this version (one entry per variant, table order).
    fn action_names() -> &'static [&'static str];

    /// Wire names of the actions a Charging Station may *originate* (Call), i.e. CS→CSMS. The
    /// client UI lists exactly these as action buttons. Includes both-direction actions (e.g.
    /// `DataTransfer`).
    fn cs_actions() -> &'static [&'static str];

    /// Wire names of the actions a CSMS may *originate* (Call), i.e. CSMS→CS, each tagged with its
    /// [`ConnectorScope`]. The server UI lists these as action buttons, filtered by the selected
    /// entry's level. This is exactly `action_names()` minus [`cs_actions()`](Version::cs_actions).
    fn csms_actions() -> &'static [(&'static str, ConnectorScope)];

    /// Build a `Default`-derived action for a wire name, as a payload template for the UI to fill.
    /// `None` for an unknown name.
    fn default_action(name: &str) -> Option<Self::Action>;

    /// Build a `Default`-derived response for a wire name, for the inbound state handler's
    /// default-accept path. `None` for an unknown name.
    fn default_response(name: &str) -> Option<Self::Response>;

    /// The websocket subprotocol token for this version (`"ocpp1.6"` / `"ocpp2.0.1"`).
    fn subprotocol() -> &'static str;

    /// Decode an inbound Call payload into a typed action, by wire action name.
    fn decode_call(action_name: &str, payload: Value) -> Result<Self::Action, OcppError>;

    /// Run `validator::Validate` on an action whose request type supports it (no-op otherwise).
    fn validate(action: &Self::Action) -> Result<(), ValidationError>;

    /// Encode our response to an inbound Call into a CallResult payload.
    fn encode_response(response: &Self::Response) -> Result<Value, OcppError>;

    /// Encode our outbound Call's request into a Call payload.
    fn encode_action(action: &Self::Action) -> Result<Value, OcppError>;

    /// Decode a CallResult payload into a typed response, using the originating action to select
    /// the response variant.
    fn decode_result(action: &Self::Action, payload: Value) -> Result<Self::Response, OcppError>;
}
