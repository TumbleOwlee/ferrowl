//! In-memory model of a Modbus register space.
//!
//! Storage is organized as [`Slice`](slice::Slice)s — contiguous runs of
//! [`Cell`] cells — grouped per device key inside a [`Memory`]. Each cell
//! tracks its register [`CellType`] (coil or register) and access [`CellKind`]
//! (read, write, or read/write), so reads and writes are validated against
//! the declared access rights of the addressed cells.

mod cell;
mod memory;
mod range;

pub mod slice;

pub use cell::{Cell, CellKind, CellType, ValueRange};
pub use memory::{Memory, MemoryError};
pub use range::Range;
