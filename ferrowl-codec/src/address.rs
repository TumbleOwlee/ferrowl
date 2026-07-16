//! Where a register sits in the device address space.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Where a register is located in the device address space.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Address {
    /// A concrete Modbus address.
    Fixed(u16),
    /// No fixed address; the register is computed or script-provided.
    Virtual,
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Address::Fixed(v) => write!(f, "{}", v),
            Address::Virtual => write!(f, "virtual"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Address;

    #[test]
    /// MB-R-003 — an address is either a fixed 16-bit Modbus address or virtual.
    fn ut_address_display() {
        assert_eq!(Address::Fixed(42).to_string(), "42");
        assert_eq!(Address::Virtual.to_string(), "virtual");
    }
}
