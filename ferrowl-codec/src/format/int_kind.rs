//! Integer primitive kind: which of the ten signed/unsigned widths a numeric format carries.

crate::format::kind_enum! {
    /// The ten integer primitive widths a [`NumericFormat`](super::NumericFormat) can carry.
    pub enum IntKind {
        U8,
        U16,
        U32,
        U64,
        U128,
        I8,
        I16,
        I32,
        I64,
        I128,
    }
}

impl IntKind {
    /// Bit width of the primitive: 8/16/32/64/128.
    pub fn bits(self) -> u32 {
        match self {
            IntKind::U8 | IntKind::I8 => 8,
            IntKind::U16 | IntKind::I16 => 16,
            IntKind::U32 | IntKind::I32 => 32,
            IntKind::U64 | IntKind::I64 => 64,
            IntKind::U128 | IntKind::I128 => 128,
        }
    }

    /// Width in 16-bit registers (an 8-bit kind still occupies one whole register).
    pub fn register_width(self) -> usize {
        (self.bits().max(16) / 16) as usize
    }
}

crate::format::display_by_variant!(IntKind {
    U8 => "U8",
    U16 => "U16",
    U32 => "U32",
    U64 => "U64",
    U128 => "U128",
    I8 => "I8",
    I16 => "I16",
    I32 => "I32",
    I64 => "I64",
    I128 => "I128",
});

#[cfg(test)]
mod tests {
    use super::IntKind;

    #[test]
    /// The bit width and register width (16-bit words) of each integer kind.
    fn ut_int_kind_bits_and_register_width() {
        assert_eq!(IntKind::U8.bits(), 8);
        assert_eq!(IntKind::U8.register_width(), 1);
        assert_eq!(IntKind::I8.bits(), 8);
        assert_eq!(IntKind::I8.register_width(), 1);
        assert_eq!(IntKind::U16.bits(), 16);
        assert_eq!(IntKind::U16.register_width(), 1);
        assert_eq!(IntKind::U32.bits(), 32);
        assert_eq!(IntKind::U32.register_width(), 2);
        assert_eq!(IntKind::U64.bits(), 64);
        assert_eq!(IntKind::U64.register_width(), 4);
        assert_eq!(IntKind::U128.bits(), 128);
        assert_eq!(IntKind::U128.register_width(), 8);
    }
}
