//! Modbus-specific module implementation: one running endpoint with its registers, shared
//! memory, log, and optional Lua simulation.
//!
//! Split into [`module`] (the `ModbusModule` struct + start/stop lifecycle), [`build`]
//! (register/memory construction from device config), and [`log`] (per-module file-sink
//! plumbing).

pub mod config;
pub mod dialog;
pub mod registers;
pub mod settings_dialog;
pub mod setup;
pub mod setup_dialog;
pub mod table;
pub mod view;

mod build;
// Lifted once run_sim plans event scripts.
#[allow(dead_code)]
mod events;
mod log;
mod module;
pub mod monitor;
mod serial_paths;

pub(crate) use events::{RegisterEvent, RegisterEventSource, RegisterTap, publish_virtual};
pub use module::{ModbusModule, ModuleLog, ModuleMemory, VirtualStore};
pub use monitor::{ModbusMonitorModule, ModbusMonitorModuleView};

pub(crate) use build::{default_value, str_to_value};
pub(crate) use log::{FileSink, append};
// `#[allow(unused_imports)]`: implemented and tested (module.rs, serial_paths.rs), but App does
// not yet attach the session-wide registry via `set_serial_paths`.
#[allow(unused_imports)]
pub(crate) use serial_paths::SerialPathRegistry;
