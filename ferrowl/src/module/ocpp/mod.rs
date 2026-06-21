//! OCPP module type.
//!
//! [`setup`] is the creation dialog; [`config`] holds the per-instance spec. The charging
//! station (client) implementation lives in [`client`]; the management-system (CSMS) role in
//! [`server`].

pub mod action_dialog;
pub mod client;
pub mod config;
pub mod server;
pub mod setup;
pub mod setup_dialog;
pub mod spec;
