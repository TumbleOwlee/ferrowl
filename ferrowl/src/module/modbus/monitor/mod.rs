//! App-side Modbus monitor build helpers (MB-R-140–MB-R-145), parallel to the existing
//! `module/modbus/build.rs` used by client/server.

mod build;
mod dialog;
mod module;
pub mod setup;
mod setup_dialog;
mod view;

#[allow(unused_imports)] // consumed by module.rs; not yet by app code outside this submodule
pub(crate) use build::{MonitorNetConfig, MonitorTransportError, endpoint_to_monitor_config};
pub use module::ModbusMonitorModule;
pub use view::ModbusMonitorModuleView;
