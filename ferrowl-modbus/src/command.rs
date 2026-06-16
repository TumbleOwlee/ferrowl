//! Commands sent to a running client task.

use tokio_modbus::SlaveId;

use crate::scalar::{Address, Coil, Word};

/// Commands sent to a running client task through its command channel.
pub enum Command {
    /// Stop the client loop.
    Terminate,
    WriteSingleCoil(SlaveId, Address, Coil),
    WriteMultipleCoils(SlaveId, Address, Vec<Coil>),
    WriteSingleRegister(SlaveId, Address, Word),
    WriteMultipleRegister(SlaveId, Address, Vec<Word>),
}
