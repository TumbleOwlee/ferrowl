//! OCPP module type — a placeholder implementation wired into the module registry.
//!
//! The setup dialog ([`setup`]) and content view ([`view`]) are intentionally minimal
//! stubs: enough to create and display a named OCPP tab. Real protocol handling (the
//! `ferrowl-ocpp` crate's `Cs`/`Csms`) is a follow-up.

pub mod setup;
pub mod view;
