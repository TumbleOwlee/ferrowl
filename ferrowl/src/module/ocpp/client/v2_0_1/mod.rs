//! OCPP 2.0.1 charging-station (client) binding: shared state (incl. transaction shortcuts +
//! variable store) and the [`crate::module::ocpp::client::view::ClientVersion`] impl wiring the
//! shared generic inbound handler ([`crate::module::ocpp::client::handler`]) into the generic view.

pub mod state;
pub mod version;
