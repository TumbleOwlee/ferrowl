use crate::range::Range;
use std::fmt::Debug;

/// The Modbus data type a memory cell holds.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum Type {
    /// Single-bit value (Modbus coil / discrete input).
    Coil,
    /// 16-bit value (Modbus holding / input register).
    Register,
}

/// A single memory cell: its [`Type`], access direction, and current value.
///
/// The variant encodes which operations are permitted: `Read` cells reject
/// checked writes, `Write` cells reject checked reads, and `ReadWrite`
/// allows both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Read-only cell.
    Read(Type, u16),
    /// Write-only cell.
    Write(Type, u16),
    /// Readable and writable cell.
    ReadWrite(Type, u16),
}

/// Access kind of a memory region: the [`Type`] plus allowed direction,
/// without a value. Used to declare ranges via [`Memory::add_ranges`](crate::Memory::add_ranges).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Read-only region.
    Read(Type),
    /// Write-only region.
    Write(Type),
    /// Readable and writable region.
    ReadWrite(Type),
}

impl Kind {
    /// Returns the underlying register [`Type`], ignoring the access direction.
    pub fn get_type(&self) -> Type {
        match self {
            Kind::Read(t) | Kind::Write(t) | Kind::ReadWrite(t) => *t,
        }
    }
}

impl Value {
    /// Creates a zero-initialized cell with the access rights of `kind`.
    pub fn default(kind: &Kind) -> Self {
        Self::from_u16(kind, 0)
    }

    /// Creates a cell with the access rights of `kind`, initialized to `init`.
    pub fn from_u16(kind: &Kind, init: u16) -> Self {
        match kind {
            Kind::Read(t) => Value::Read(*t, init),
            Kind::Write(t) => Value::Write(*t, init),
            Kind::ReadWrite(t) => Value::ReadWrite(*t, init),
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
    use super::{Kind, Type, Value, ValueRange};

    #[test]
    fn ut_value_default() {
        assert_eq!(
            Value::default(&Kind::Read(Type::Coil)),
            Value::Read(Type::Coil, 0)
        );
        assert_eq!(
            Value::default(&Kind::Write(Type::Coil)),
            Value::Write(Type::Coil, 0)
        );
        assert_eq!(
            Value::default(&Kind::ReadWrite(Type::Coil)),
            Value::ReadWrite(Type::Coil, 0)
        );
    }

    #[test]
    fn ut_value_from_u16() {
        assert_eq!(
            Value::from_u16(&Kind::Read(Type::Coil), 1),
            Value::Read(Type::Coil, 1)
        );
        assert_eq!(
            Value::from_u16(&Kind::Write(Type::Coil), 2),
            Value::Write(Type::Coil, 2)
        );
        assert_eq!(
            Value::from_u16(&Kind::ReadWrite(Type::Coil), 3),
            Value::ReadWrite(Type::Coil, 3)
        );
    }

    #[test]
    fn ut_value_range_new() {
        let values: Vec<u16> = (1..21).collect();
        let range = ValueRange::new(100, &values);

        assert_eq!(range.range.start, 100);
        assert_eq!(range.range.end, 120);
    }
}
