//! App-side Modbus monitor build helpers (MB-R-140–MB-R-145), parallel to the existing
//! `module/modbus/build.rs` used by client/server.
//!
//! Forward-declared: the `ModbusMonitorModule` lifecycle wrapper that consumes
//! `endpoint_to_monitor_config` lands in s4 of the modbus-bus-monitor plan.

mod build;
mod dialog;
mod module;
pub mod setup;
mod setup_dialog;
mod view;

#[allow(unused_imports)] // consumed by view/mod.rs's Add command wiring
pub(crate) use dialog::AddInterpretationDialog;

#[allow(unused_imports)] // consumed by module.rs; not yet by app code outside this submodule
pub(crate) use build::{MonitorNetConfig, MonitorTransportError, endpoint_to_monitor_config};
pub use module::ModbusMonitorModule;
pub use view::ModbusMonitorModuleView;
