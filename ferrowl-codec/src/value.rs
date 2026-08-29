//! Typed values decoded from raw register words.

use crate::format::Resolution;

/// The numeric primitive a decoded value or unscaled value carries, shared
/// between [`Value`] and [`UnscaledValue`].
#[derive(Debug, Clone)]
pub enum NumericPrimitive {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    F32(f32),
    F64(f64),
}

impl std::fmt::Display for NumericPrimitive {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Every variant just forwards its inner value's own Display.
        macro_rules! disp {
            ($v:expr) => {
                write!(fmt, "{}", $v)
            };
        }
        match self {
            Self::U8(v) => disp!(v),
            Self::U16(v) => disp!(v),
            Self::U32(v) => disp!(v),
            Self::U64(v) => disp!(v),
            Self::U128(v) => disp!(v),
            Self::I8(v) => disp!(v),
            Self::I16(v) => disp!(v),
            Self::I32(v) => disp!(v),
            Self::I64(v) => disp!(v),
            Self::I128(v) => disp!(v),
            Self::F32(v) => disp!(v),
            Self::F64(v) => disp!(v),
        }
    }
}

impl NumericPrimitive {
    /// The value widened to `f64`, used for display scaling (`raw × resolution`).
    fn as_f64(&self) -> f64 {
        macro_rules! wide {
            ($v:expr) => {
                *$v as f64
            };
        }
        match self {
            Self::U8(v) => wide!(v),
            Self::U16(v) => wide!(v),
            Self::U32(v) => wide!(v),
            Self::U64(v) => wide!(v),
            Self::U128(v) => wide!(v),
            Self::I8(v) => wide!(v),
            Self::I16(v) => wide!(v),
            Self::I32(v) => wide!(v),
            Self::I64(v) => wide!(v),
            Self::I128(v) => wide!(v),
            Self::F32(v) => wide!(v),
            Self::F64(v) => *v,
        }
    }

    /// Formats the raw value as `0x`-prefixed, zero-padded hex (two's complement
    /// for signed, IEEE 754 bits for floats), width = 2 hex digits per byte.
    fn as_hex_str(&self) -> String {
        macro_rules! hex {
            ($v:expr, $width:expr) => {
                format!("0x{:01$X}", $v, $width)
            };
        }
        match self {
            Self::U8(v) => hex!(v, 2),
            Self::U16(v) => hex!(v, 4),
            Self::U32(v) => hex!(v, 8),
            Self::U64(v) => hex!(v, 16),
            Self::U128(v) => hex!(v, 32),
            Self::I8(v) => hex!(v, 2),
            Self::I16(v) => hex!(v, 4),
            Self::I32(v) => hex!(v, 8),
            Self::I64(v) => hex!(v, 16),
            Self::I128(v) => hex!(v, 32),
            Self::F32(v) => hex!(v.to_bits(), 8),
            Self::F64(v) => hex!(v.to_bits(), 16),
        }
    }
}

/// A decoded value with the display [`Resolution`] dropped: the bare numeric
/// primitive as it sits on the wire, or ASCII text. Produced by
/// [`Value::unscaled`].
#[derive(Debug, Clone)]
pub enum UnscaledValue {
    Numeric(NumericPrimitive),
    Ascii(String),
}

impl std::fmt::Display for UnscaledValue {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Numeric(p) => write!(fmt, "{p}"),
            Self::Ascii(s) => write!(fmt, "{s}"),
        }
    }
}

/// A decoded register value: the typed raw value plus, for numeric variants,
/// the display [`Resolution`] it is scaled by when formatted.
#[derive(Debug, Clone)]
pub enum Value {
    Numeric(NumericPrimitive, Resolution),
    Ascii(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Numeric(p, r) => {
                let v = p.as_f64() * r.0;
                write!(fmt, "{v}")
            }
            Self::Ascii(v) => write!(fmt, "{v}"),
        }
    }
}

impl Value {
    pub fn unscaled(self) -> UnscaledValue {
        match self {
            Self::Numeric(p, _) => UnscaledValue::Numeric(p),
            Self::Ascii(s) => UnscaledValue::Ascii(s),
        }
    }

    /// `true` only for an empty ASCII value — the "no value yet" sentinel;
    /// numeric variants always carry a value.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Ascii(s) if s.is_empty())
    }

    /// Formats the unscaled raw value as `0x`-prefixed, zero-padded hex
    /// (two's complement for signed, IEEE 754 bits for floats, one byte per
    /// character for ASCII).
    pub fn as_hex_str(&self) -> String {
        match self {
            Self::Numeric(p, _) => p.as_hex_str(),
            Self::Ascii(v) => {
                let bytes = v.as_bytes();
                let mut str = "0x".to_string();
                for b in bytes.iter() {
                    str += &format!("{:01$X}", b, 2);
                }
                str
            }
        }
    }

    pub fn u8(v: u8, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::U8(v), r)
    }
    pub fn u16(v: u16, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::U16(v), r)
    }
    pub fn u32(v: u32, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::U32(v), r)
    }
    pub fn u64(v: u64, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::U64(v), r)
    }
    pub fn u128(v: u128, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::U128(v), r)
    }
    pub fn i8(v: i8, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::I8(v), r)
    }
    pub fn i16(v: i16, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::I16(v), r)
    }
    pub fn i32(v: i32, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::I32(v), r)
    }
    pub fn i64(v: i64, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::I64(v), r)
    }
    pub fn i128(v: i128, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::I128(v), r)
    }
    pub fn f32(v: f32, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::F32(v), r)
    }
    pub fn f64(v: f64, r: Resolution) -> Self {
        Self::Numeric(NumericPrimitive::F64(v), r)
    }
}

#[cfg(test)]
mod tests {
    use super::{NumericPrimitive, Value};
    use crate::format::Resolution;

    fn res() -> Resolution {
        Resolution(1.0)
    }

    #[test]
    /// MB-R-021 — displaying a value yields `raw × resolution`; resolution 1.0 leaves it unchanged.
    fn ut_value_as_str_no_scaling() {
        assert_eq!(Value::u8(42, res()).to_string(), "42");
        assert_eq!(Value::u16(1000, res()).to_string(), "1000");
        assert_eq!(Value::i8(-1, res()).to_string(), "-1");
        assert_eq!(Value::i16(-100, res()).to_string(), "-100");
        assert_eq!(Value::Ascii("hello".to_string()).to_string(), "hello");
    }

    #[test]
    /// MB-R-021 — displaying a value applies the display resolution as `raw × resolution`.
    fn ut_value_as_str_with_scaling() {
        // Use resolution 2.0 so that integer * 2.0 is exact in f64
        let r = Resolution(2.0);
        assert_eq!(Value::u16(5, r.clone()).to_string(), "10");
        assert_eq!(Value::i32(-3, r.clone()).to_string(), "-6");
        assert_eq!(Value::f32(1.5f32, r.clone()).to_string(), "3");
    }

    #[test]
    /// MB-R-021 — the unscaled value is the raw value, with the resolution not applied.
    fn ut_value_unscaled_drops_resolution() {
        // The unscaled string is the raw value, regardless of resolution.
        let r = Resolution(2.0);
        assert_eq!(Value::u16(5, r.clone()).unscaled().to_string(), "5");
        assert_eq!(Value::i32(-3, r.clone()).unscaled().to_string(), "-3");
        assert_eq!(Value::f32(1.5f32, r.clone()).unscaled().to_string(), "1.5");
        assert_eq!(
            Value::Ascii("hello".to_string()).unscaled().to_string(),
            "hello"
        );
    }

    #[test]
    fn ut_value_is_empty() {
        // Only the empty ASCII sentinel counts as empty.
        assert!(Value::Ascii(String::new()).is_empty());
        assert!(!Value::Ascii("x".to_string()).is_empty());
        assert!(!Value::u16(0, res()).is_empty());
    }

    #[test]
    /// MB-R-025 — a value renders as raw zero-padded hex (two's complement for signed, one byte per ASCII char).
    fn ut_value_as_hex_str() {
        assert_eq!(Value::u8(0xFF, res()).as_hex_str(), "0xFF");
        assert_eq!(Value::u16(0x1234, res()).as_hex_str(), "0x1234");
        assert_eq!(Value::u32(0x12345678, res()).as_hex_str(), "0x12345678");
        assert_eq!(Value::u64(0, res()).as_hex_str(), "0x0000000000000000");
        // Negative i8 formatted as bit-pattern hex: -1i8 as u8 = 0xFF
        assert_eq!(Value::i8(-1i8, res()).as_hex_str(), "0xFF");
        assert_eq!(Value::i16(-1i16, res()).as_hex_str(), "0xFFFF");
        // ASCII: each byte represented as 2 hex digits
        assert_eq!(Value::Ascii("AB".to_string()).as_hex_str(), "0x4142");
    }

    #[test]
    /// MB-R-025 — a float renders as its IEEE 754 bit pattern in zero-padded hex.
    fn ut_value_as_hex_str_f32() {
        let bits = 1.5f32.to_bits();
        let expected = format!("0x{bits:08X}");
        assert_eq!(Value::f32(1.5f32, res()).as_hex_str(), expected);
    }

    #[test]
    /// MB-R-025 — an f64 renders as its IEEE 754 bit pattern in zero-padded hex.
    fn ut_value_as_hex_str_f64() {
        let bits = 1.5f64.to_bits();
        let expected = format!("0x{bits:016X}");
        assert_eq!(Value::f64(1.5f64, res()).as_hex_str(), expected);
    }

    #[test]
    /// MB-R-021 — the unscaled value displays the raw value for every variant.
    fn ut_unscaled_value_display_all_variants() {
        use super::UnscaledValue;
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::U8(8)).to_string(),
            "8"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::U16(16)).to_string(),
            "16"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::U32(32)).to_string(),
            "32"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::U64(64)).to_string(),
            "64"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::U128(128)).to_string(),
            "128"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::I8(-8)).to_string(),
            "-8"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::I16(-16)).to_string(),
            "-16"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::I32(-32)).to_string(),
            "-32"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::I64(-64)).to_string(),
            "-64"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::I128(-128)).to_string(),
            "-128"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::F32(1.5)).to_string(),
            "1.5"
        );
        assert_eq!(
            UnscaledValue::Numeric(NumericPrimitive::F64(2.5)).to_string(),
            "2.5"
        );
        assert_eq!(UnscaledValue::Ascii("hi".to_string()).to_string(), "hi");
    }

    #[test]
    /// MB-R-021 — every numeric variant displays as `raw × resolution`.
    fn ut_value_display_all_numeric_variants() {
        // Resolution 1.0 keeps the scaled value equal to the raw value.
        assert_eq!(Value::u32(32, res()).to_string(), "32");
        assert_eq!(Value::u64(64, res()).to_string(), "64");
        assert_eq!(Value::u128(128, res()).to_string(), "128");
        assert_eq!(Value::i64(-64, res()).to_string(), "-64");
        assert_eq!(Value::i128(-128, res()).to_string(), "-128");
        assert_eq!(Value::f64(2.5, res()).to_string(), "2.5");
    }

    #[test]
    /// MB-R-021 — `unscaled` preserves the raw value and variant for every type.
    fn ut_value_unscaled_all_variants() {
        use super::UnscaledValue;
        assert!(matches!(
            Value::u8(8, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::U8(8))
        ));
        assert!(matches!(
            Value::u32(32, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::U32(32))
        ));
        assert!(matches!(
            Value::u64(64, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::U64(64))
        ));
        assert!(matches!(
            Value::u128(128, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::U128(128))
        ));
        assert!(matches!(
            Value::i8(-8, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::I8(-8))
        ));
        assert!(matches!(
            Value::i16(-16, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::I16(-16))
        ));
        assert!(matches!(
            Value::i64(-64, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::I64(-64))
        ));
        assert!(matches!(
            Value::i128(-128, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::I128(-128))
        ));
        assert!(matches!(
            Value::f64(2.5, res()).unscaled(),
            UnscaledValue::Numeric(NumericPrimitive::F64(_))
        ));
    }

    #[test]
    /// MB-R-025 — wide and signed variants render as raw zero-padded two's-complement hex.
    fn ut_value_as_hex_str_remaining_variants() {
        assert_eq!(
            Value::u128(0x1, res()).as_hex_str(),
            "0x00000000000000000000000000000001"
        );
        assert_eq!(Value::i32(-1i32, res()).as_hex_str(), "0xFFFFFFFF");
        assert_eq!(Value::i64(-1i64, res()).as_hex_str(), "0xFFFFFFFFFFFFFFFF");
        assert_eq!(
            Value::i128(-1i128, res()).as_hex_str(),
            "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"
        );
    }
}
