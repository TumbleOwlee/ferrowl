use crate::range::Range;
use std::fmt::Debug;

/// The Modbus data type a memory cell holds.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum CellType {
    /// Single-bit value (Modbus coil / discrete input).
    Coil,
    /// 16-bit value (Modbus holding / input register).
    Register,
}

/// A single memory cell: its [`CellType`], access direction, and current value.
///
/// The variant encodes which operations are permitted: `Read` cells reject
/// checked writes, `Write` cells reject checked reads, and `ReadWrite`
/// allows both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// Read-only cell.
    Read(CellType, u16),
    /// Write-only cell.
    Write(CellType, u16),
    /// Readable and writable cell.
    ReadWrite(CellType, u16),
}

/// Access kind of a memory region: the [`CellType`] plus allowed direction,
/// without a value. Used to declare ranges via [`Memory::add_ranges`](crate::Memory::add_ranges).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellKind {
    /// Read-only region.
    Read(CellType),
    /// Write-only region.
    Write(CellType),
    /// Readable and writable region.
    ReadWrite(CellType),
}

impl CellKind {
    /// Returns the underlying register [`CellType`], ignoring the access direction.
    pub fn cell_type(&self) -> CellType {
        match self {
            CellKind::Read(t) | CellKind::Write(t) | CellKind::ReadWrite(t) => *t,
        }
    }
}

impl Cell {
    /// Creates a zero-initialized cell with the access rights of `kind`.
    pub fn default(kind: &CellKind) -> Self {
        Self::from_u16(kind, 0)
    }

    /// Creates a cell with the access rights of `kind`, initialized to `init`.
    pub fn from_u16(kind: &CellKind, init: u16) -> Self {
        match kind {
            CellKind::Read(t) => Cell::Read(*t, init),
            CellKind::Write(t) => Cell::Write(*t, init),
            CellKind::ReadWrite(t) => Cell::ReadWrite(*t, init),
        }
    }

    /// Returns `true` if this cell accepts checked writes of type `ty`.
    pub fn accepts_write(&self, ty: &CellType) -> bool {
        matches!(self, Cell::Write(t, _) | Cell::ReadWrite(t, _) if t == ty)
    }

    /// Returns `true` if this cell accepts checked reads of type `ty`.
    pub fn accepts_read(&self, ty: &CellType) -> bool {
        matches!(self, Cell::Read(t, _) | Cell::ReadWrite(t, _) if t == ty)
    }

    /// Returns the stored value regardless of access rights.
    pub fn value(&self) -> u16 {
        match self {
            Cell::Read(_, v) | Cell::Write(_, v) | Cell::ReadWrite(_, v) => *v,
        }
    }

    /// Sets the stored value regardless of access rights.
    pub fn set_value(&mut self, val: u16) {
        match self {
            Cell::Read(_, v) | Cell::Write(_, v) | Cell::ReadWrite(_, v) => *v = val,
        }
    }

    /// Sets the stored value if this cell accepts writes; leaves read-only
    /// cells untouched.
    pub fn try_set_value(&mut self, val: u16) {
        match self {
            Cell::Write(_, v) | Cell::ReadWrite(_, v) => *v = val,
            Cell::Read(_, _) => {}
        }
    }

    /// Returns the stored value if this cell accepts reads, `None` for
    /// write-only cells.
    pub fn try_value(&self) -> Option<u16> {
        match self {
            Cell::Read(_, v) | Cell::ReadWrite(_, v) => Some(*v),
            Cell::Write(_, _) => None,
        }
    }
}

/// A borrowed run of raw `u16` values paired with the address [`Range`]
/// they occupy. The range length always equals the number of values.
#[derive(Debug, Clone)]
pub struct ValueRange<'a> {
    range: Range,
    values: &'a [u16],
}

impl<'a> ValueRange<'a> {
    /// Creates a value range starting at address `start`; the range end is
    /// derived from the slice length.
    pub fn new(start: usize, values: &'a [u16]) -> Self {
        Self {
            range: Range::new(start, values.len()),
            values,
        }
    }

    /// Returns the raw values.
    pub fn get_values(&self) -> &'a [u16] {
        self.values
    }

    /// Returns the address range covered by the values.
    pub fn get_range(&self) -> Range {
        self.range.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, CellKind, CellType, ValueRange};

    #[test]
    fn ut_value_default() {
        assert_eq!(
            Cell::default(&CellKind::Read(CellType::Coil)),
            Cell::Read(CellType::Coil, 0)
        );
        assert_eq!(
            Cell::default(&CellKind::Write(CellType::Coil)),
            Cell::Write(CellType::Coil, 0)
        );
        assert_eq!(
            Cell::default(&CellKind::ReadWrite(CellType::Coil)),
            Cell::ReadWrite(CellType::Coil, 0)
        );
    }

    #[test]
    fn ut_value_from_u16() {
        assert_eq!(
            Cell::from_u16(&CellKind::Read(CellType::Coil), 1),
            Cell::Read(CellType::Coil, 1)
        );
        assert_eq!(
            Cell::from_u16(&CellKind::Write(CellType::Coil), 2),
            Cell::Write(CellType::Coil, 2)
        );
        assert_eq!(
            Cell::from_u16(&CellKind::ReadWrite(CellType::Coil), 3),
            Cell::ReadWrite(CellType::Coil, 3)
        );
    }

    #[test]
    fn ut_value_range_new() {
        let values: Vec<u16> = (1..21).collect();
        let range = ValueRange::new(100, &values);

        assert_eq!(range.range.start, 100);
        assert_eq!(range.range.end, 120);
    }

    #[test]
    fn ut_kind_get_type() {
        assert_eq!(CellKind::Read(CellType::Coil).cell_type(), CellType::Coil);
        assert_eq!(
            CellKind::Write(CellType::Register).cell_type(),
            CellType::Register
        );
        assert_eq!(
            CellKind::ReadWrite(CellType::Coil).cell_type(),
            CellType::Coil
        );
    }

    #[test]
    fn ut_value_range_accessors() {
        let values: Vec<u16> = vec![7, 8, 9];
        let range = ValueRange::new(50, &values);
        assert_eq!(range.get_values(), &[7, 8, 9]);
        assert_eq!(range.get_range(), crate::range::Range::new(50, 3));
    }
}
