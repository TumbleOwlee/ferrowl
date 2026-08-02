//! A single recurring Modbus operation performed each poll cycle.

use ferrowl_store::Range;
use rust_modbus::{FunctionCode, UnitId};

/// A single recurring Modbus operation a client performs each poll cycle:
/// the function code applied to an address range on a slave.
#[derive(Debug, Clone)]
pub struct Operation {
    pub slave_id: UnitId,
    pub fn_code: FunctionCode,
    pub range: Range,
}
