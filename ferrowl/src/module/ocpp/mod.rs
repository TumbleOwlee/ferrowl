//! OCPP module type.
//!
//! [`setup`] is the creation dialog; [`config`] holds the per-instance spec. The charging
//! station (client) implementation lives in [`client`]; the server (CSMS) role uses the
//! placeholder in [`view`].

pub mod client;
pub mod config;
pub mod setup;
pub mod setup_dialog;
pub mod view;
