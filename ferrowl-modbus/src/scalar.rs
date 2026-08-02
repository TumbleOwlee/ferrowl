//! Primitive wire-scalar types used across the Modbus API.
//!
//! Addresses and register values are the protocol library's own newtypes, so one
//! cannot be passed where the other is meant. A coil stays a bare `bool`: the
//! library takes and returns `bool` for single-bit data.

pub use rust_modbus::{Address, RegisterValue as Word};

/// A coil (single-bit) value.
pub type Coil = bool;
