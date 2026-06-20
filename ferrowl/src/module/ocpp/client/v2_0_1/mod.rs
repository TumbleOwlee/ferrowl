//! OCPP 2.0.1 charging-station (client) UI: system-state panel, action buttons (incl. transaction
//! shortcuts), variable store, message log.

pub mod handler;
pub mod state;
pub mod view;

pub use view::OcppClientV201View;
