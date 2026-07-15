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

    /// Whether `mask` fits within the low `bits` bits, i.e. sets no bit at or
    /// above position `bits`. `bits >= 128` always fits (`u128`/`i128`). The
    /// full-width default (`u128::MAX`) is always considered to fit, since it
    /// is the no-op sentinel narrowed to any format's width when applied.
    pub fn fits(&self, bits: u32) -> bool {
        if self.is_full() {
            return true;
        }
        let limit = if bits >= 128 {
            u128::MAX
        } else {
            (1u128 << bits) - 1
        };
        self.mask <= limit
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

    /// MB-R-014 — the field shift is derived from the mask as its trailing-zero count;
    /// the default mask is the full-width no-op.
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

    /// MB-R-016 — a mask setting any bit at or above the format width is rejected; the
    /// full-width default mask always fits.
    #[test]
    fn ut_bitfield_fits() {
        assert!(BitField { mask: 0xFF }.fits(8));
        assert!(!BitField { mask: 0x1FF }.fits(8));
        assert!(!BitField { mask: 0xFF00 }.fits(8));
        assert!(BitField { mask: u128::MAX }.fits(128));
        // The full-width sentinel always fits: it is narrowed to each
        // format's own width when applied, regardless of `bits`.
        assert!(BitField::default().fits(128));
        assert!(BitField::default().fits(8));
    }
}
