//! Modbus-specific module implementation: one running endpoint with its registers, shared
//! memory, log, and optional Lua simulation.
//!
//! Split into [`module`] (the `ModbusModule` struct + start/stop lifecycle), [`build`]
//! (register/memory construction from device config), and [`log`] (per-module file-sink
//! plumbing).

pub mod config;
pub mod dialog;
pub mod registers;
pub mod setup;
pub mod setup_dialog;
pub mod table;
pub mod view;

mod build;
mod log;
mod module;

pub use module::{ModbusModule, ModuleLog, ModuleMemory, VirtualStore};

pub(crate) use build::{default_value, str_to_value};
pub(crate) use log::{FileSink, append};
