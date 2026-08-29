//! Float primitive kind: which of the two IEEE 754 widths a float format carries.

crate::format::kind_enum! {
    /// The two float primitive widths a [`FloatFormat`](super::FloatFormat) can carry.
    pub enum FloatKind {
        F32,
        F64,
    }
}

impl FloatKind {
    /// Bit width of the primitive: 32/64.
    pub fn bits(self) -> u32 {
        match self {
            FloatKind::F32 => 32,
            FloatKind::F64 => 64,
        }
    }

    /// Width in 16-bit registers.
    pub fn register_width(self) -> usize {
        (self.bits() / 16) as usize
    }
}

crate::format::display_by_variant!(FloatKind {
    F32 => "F32",
    F64 => "F64",
});

#[cfg(test)]
mod tests {
    use super::FloatKind;

    #[test]
    /// The bit width and register width (16-bit words) of each float kind.
    fn ut_float_kind_bits_and_register_width() {
        assert_eq!(FloatKind::F32.bits(), 32);
        assert_eq!(FloatKind::F32.register_width(), 2);
        assert_eq!(FloatKind::F64.bits(), 64);
        assert_eq!(FloatKind::F64.register_width(), 4);
    }
}
