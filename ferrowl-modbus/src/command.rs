//! Commands sent to a running client task.

use rust_modbus::UnitId;

use crate::scalar::{Address, Coil, Word};

/// Commands sent to a running client task through its command channel.
pub enum Command {
    /// Stop the client loop.
    Terminate,
    WriteSingleCoil(UnitId, Address, Coil),
    WriteMultipleCoils(UnitId, Address, Vec<Coil>),
    WriteSingleRegister(UnitId, Address, Word),
    WriteMultipleRegister(UnitId, Address, Vec<Word>),
}
