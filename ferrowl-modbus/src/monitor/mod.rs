//! MB-R-140–MB-R-145 — a Modbus monitor: a receive-only observer of an `Rtu`/`Ascii` serial
//! bus, decoding traffic produced entirely by other devices' own client/server exchanges,
//! never itself initiating a transaction (MB-R-141).

mod core;
mod table;

pub use table::{ObservedTable, SharedObservedTable};

pub(crate) use core::{MonitorEnd, drive_monitor};
