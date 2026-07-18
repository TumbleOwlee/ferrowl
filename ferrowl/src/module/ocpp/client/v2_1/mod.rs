//! OCPP 2.1 charging-station (client) binding. 2.1 is a strict superset of 2.0.1 and the simulator
//! behaves identically for the core Calls, so this version reuses 2.0.1's shared state
//! ([`crate::module::ocpp::client::v2_0_1::state`]), the shared generic inbound handler
//! ([`crate::module::ocpp::client::handler`]) and the shared `ClientVersion` body
//! ([`crate::module::ocpp::client::v2_common`]), instantiated here for `V2_1`.

pub mod inbound;
pub mod version;
