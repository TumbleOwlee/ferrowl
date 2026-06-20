//! OCPP 1.6 charging-station (client) UI: system-state panel, action buttons, message log.

pub mod config_dialog;
pub mod handler;
pub mod state;
pub mod view;

pub use view::OcppClientV16View;
