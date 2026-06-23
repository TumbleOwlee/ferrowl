//! OCPP 2.0.1 charging-station (client) binding: shared state (incl. transaction shortcuts +
//! variable store), the inbound handler, and the
//! [`crate::module::ocpp::client::view::ClientVersion`] impl wiring it into the generic view.

pub mod handler;
pub mod state;
pub mod version;
