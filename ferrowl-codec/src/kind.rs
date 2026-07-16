//! The Modbus register table a value lives in.

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

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    /// MB-R-004 — the four Modbus register tables (coil, discrete input, holding
    /// register, input register) are exactly the values a kind can take.
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
}
