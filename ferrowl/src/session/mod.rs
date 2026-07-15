//! Session-level Lua simulation: the [`SessionSim`] sim thread that drives cross-module scripts
//! against a real [`ModuleRegistry`](crate::registry::ModuleRegistry), plus its end-to-end tests.

mod sim;
pub use sim::SessionSim;

#[cfg(test)]
mod e2e_tests;
