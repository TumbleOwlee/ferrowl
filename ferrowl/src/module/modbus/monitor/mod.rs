//! App-side Modbus monitor build helpers (MB-R-140–MB-R-145), parallel to the existing
//! `module/modbus/build.rs` used by client/server.

mod build;
mod dialog;
mod module;
pub mod setup;
mod setup_dialog;
mod view;

pub use module::ModbusMonitorModule;
pub use view::ModbusMonitorModuleView;
