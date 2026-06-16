//! Bit-field selector for integer values.

use serde::{Deserialize, Serialize};

/// Bit-field selector for an integer value: the raw word is masked and the
/// resulting field shifted down by the mask's trailing-zero count.
///
/// Read: `field = (raw & mask) >> shift`. Write: `raw = (value << shift) & mask`.
/// The shift is *derived* from `mask` (the bit position of its least-significant
/// set bit), so only the mask is ever configured. The default (`u128::MAX`,
/// shift 0) is a no-op and is narrowed to each format's own width when applied.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BitField {
    pub mask: u128,
}

impl Default for BitField {
    fn default() -> Self {
        Self { mask: u128::MAX }
    }
}

impl BitField {
    /// Bit offset of the field, i.e. the mask's trailing-zero count (0 for a
    /// zero mask, which is otherwise degenerate).
    pub fn shift(&self) -> u32 {
        if self.mask == 0 {
            0
        } else {
            self.mask.trailing_zeros()
        }
    }

    /// Whether this is the no-op full mask (all bits selected, shift 0).
    pub fn is_full(&self) -> bool {
        self.mask == u128::MAX
    }
}

impl std::fmt::Display for BitField {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "0x{:X}", self.mask)
    }
}

#[cfg(test)]
mod tests {
    use super::BitField;

    #[test]
    fn ut_bitfield_default_shift_is_full_display() {
        assert_eq!(BitField::default().mask, u128::MAX);
        assert!(BitField::default().is_full());
        // Zero mask is the degenerate case: shift is 0, not a panic.
        assert_eq!(BitField { mask: 0 }.shift(), 0);
        assert_eq!(BitField { mask: 0xFF00 }.shift(), 8);
        assert!(!BitField { mask: 0xFF00 }.is_full());
        assert_eq!(BitField { mask: 0xABCD }.to_string(), "0xABCD");
    }
}
