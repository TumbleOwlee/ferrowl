//! In-memory model of a Modbus register space.
//!
//! Storage is organized as [`Slice`](slice::Slice)s — contiguous runs of
//! [`Value`] cells — grouped per device key inside a [`Memory`]. Each cell
//! tracks its register [`Type`] (coil or register) and access [`Kind`]
//! (read, write, or read/write), so reads and writes are validated against
//! the declared access rights of the addressed cells.

mod memory;
mod range;
mod value;

pub mod slice;

pub use memory::Memory;
pub use range::Range;
pub use value::{Kind, Type, Value, ValueRange};
