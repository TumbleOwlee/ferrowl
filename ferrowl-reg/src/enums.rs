//! Enums describing a register's table, address, and access rights.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// The four Modbus register tables a value can live in.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Kind {
    /// Single-bit, read/write (function codes 1, 5, 15).
    Coil,
    /// Single-bit, read-only (function code 2).
    DiscreteInput,
    /// 16-bit, read/write (function codes 3, 6, 16).
    #[default]
    HoldingRegister,
    /// 16-bit, read-only (function code 4).
    InputRegister,
}

impl Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Kind::Coil => write!(f, "Coil"),
            Kind::DiscreteInput => write!(f, "Discrete Input"),
            Kind::HoldingRegister => write!(f, "Holding Register"),
            Kind::InputRegister => write!(f, "Input Register"),
        }
    }
}

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

/// Allowed access direction for a register.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Register can only be read.
    ReadOnly,
    /// Register can only be written.
    WriteOnly,
    /// Register can be read and written.
    ReadWrite,
}

impl Display for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Access::ReadOnly => write!(f, "ReadOnly"),
            Access::WriteOnly => write!(f, "WriteOnly"),
            Access::ReadWrite => write!(f, "ReadWrite"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Access, Address, Kind};

    #[test]
    fn ut_kind_display() {
        assert_eq!(Kind::Coil.to_string(), "Coil");
        assert_eq!(Kind::DiscreteInput.to_string(), "Discrete Input");
        assert_eq!(Kind::HoldingRegister.to_string(), "Holding Register");
        assert_eq!(Kind::InputRegister.to_string(), "Input Register");
    }

    #[test]
    fn ut_kind_default() {
        assert_eq!(Kind::default(), Kind::HoldingRegister);
    }

    #[test]
    fn ut_address_display() {
        assert_eq!(Address::Fixed(42).to_string(), "42");
        assert_eq!(Address::Virtual.to_string(), "virtual");
    }

    #[test]
    fn ut_access_display() {
        assert_eq!(Access::ReadOnly.to_string(), "ReadOnly");
        assert_eq!(Access::WriteOnly.to_string(), "WriteOnly");
        assert_eq!(Access::ReadWrite.to_string(), "ReadWrite");
    }
}
