//! App-side Modbus monitor build helpers (MB-R-140–MB-R-145), parallel to the existing
//! `module/modbus/build.rs` used by client/server.
//!
//! Forward-declared: the `ModbusMonitorModule` lifecycle wrapper that consumes
//! `endpoint_to_monitor_config` lands in s4 of the modbus-bus-monitor plan.

mod build;

#[allow(unused_imports)] // consumed starting s4; see module doc
pub(crate) use build::{MonitorNetConfig, MonitorTransportError, endpoint_to_monitor_config};
