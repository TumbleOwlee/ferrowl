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

/// Commands sent to a running server task through its command channel. Deliberately smaller
/// than [`Command`]: a server never executes a write, so a caller holding only a server's
/// command sender has no [`Command`] variant to send through it — MB-R-093 stays true by
/// construction, not by a runtime check.
pub enum ServerCommand {
    /// Stop the server loop.
    Terminate,
}
