//! Primitive wire-scalar aliases used across the Modbus API.

/// A Modbus register address.
pub type Address = u16;
/// A raw 16-bit register value.
pub type Word = u16;
/// A coil (single-bit) value.
pub type Coil = bool;
