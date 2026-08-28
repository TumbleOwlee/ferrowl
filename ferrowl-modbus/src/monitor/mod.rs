//! MB-R-140–MB-R-148 — a Modbus monitor: a receive-only observer of an `Rtu`/`Ascii` serial
//! bus, decoding traffic produced entirely by other devices' own client/server exchanges,
//! never itself initiating a transaction (MB-R-141).

mod core;
mod record;
mod table;

pub use record::{
    MonitorRecord, RECORD_RING_CAPACITY, RecordLog, RecordStatus, SharedRecordLog, TableShape,
    recency_active_at,
};
pub use table::{ObservedTable, SharedObservedTable};

pub(crate) use core::run_serial_monitor;
