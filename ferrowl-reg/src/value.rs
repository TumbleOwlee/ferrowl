//! Typed values decoded from raw register words.

use serde::{Deserialize, Serialize};

use crate::format::Resolution;

/// A decoded register value: the typed raw value plus.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum UnscaledValue {
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
    Ascii(String),
}

impl std::fmt::Display for UnscaledValue {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::U8(v) => write!(fmt, "{}", v),
            Self::U16(v) => write!(fmt, "{}", v),
            Self::U32(v) => write!(fmt, "{}", v),
            Self::U64(v) => write!(fmt, "{}", v),
            Self::U128(v) => write!(fmt, "{}", v),
            Self::I8(v) => write!(fmt, "{}", v),
            Self::I16(v) => write!(fmt, "{}", v),
            Self::I32(v) => write!(fmt, "{}", v),
            Self::I64(v) => write!(fmt, "{}", v),
            Self::I128(v) => write!(fmt, "{}", v),
            Self::F32(v) => write!(fmt, "{}", v),
            Self::F64(v) => write!(fmt, "{}", v),
            Self::Ascii(v) => write!(fmt, "{}", v),
        }
    }
}

/// A decoded register value: the typed raw value plus, for numeric variants,
/// the display [`Resolution`] it is scaled by when formatted.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Value {
    U8((u8, Resolution)),
    U16((u16, Resolution)),
    U32((u32, Resolution)),
    U64((u64, Resolution)),
    U128((u128, Resolution)),
    I8((i8, Resolution)),
    I16((i16, Resolution)),
    I32((i32, Resolution)),
    I64((i64, Resolution)),
    I128((i128, Resolution)),
    F32((f32, Resolution)),
    F64((f64, Resolution)),
    Ascii(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = {
            // Every numeric variant scales by its resolution and prints the f64 result.
            macro_rules! scaled {
                ($v:expr, $r:expr) => {{
                    let v = *$v as f64 * $r.0;
                    format!("{v}")
                }};
            }
            match self {
                Self::U8((v, r)) => scaled!(v, r),
                Self::U16((v, r)) => scaled!(v, r),
                Self::U32((v, r)) => scaled!(v, r),
                Self::U64((v, r)) => scaled!(v, r),
                Self::U128((v, r)) => scaled!(v, r),
                Self::I8((v, r)) => scaled!(v, r),
                Self::I16((v, r)) => scaled!(v, r),
                Self::I32((v, r)) => scaled!(v, r),
                Self::I64((v, r)) => scaled!(v, r),
                Self::I128((v, r)) => scaled!(v, r),
                Self::F32((v, r)) => scaled!(v, r),
                Self::F64((v, r)) => scaled!(v, r),
                Self::Ascii(v) => v.chars().collect(),
            }
        };
        write!(fmt, "{}", val)
    }
}

impl Value {
    pub fn unscaled(self) -> UnscaledValue {
        match self {
            Self::U8((v, _r)) => UnscaledValue::U8(v),
            Self::U16((v, _r)) => UnscaledValue::U16(v),
            Self::U32((v, _r)) => UnscaledValue::U32(v),
            Self::U64((v, _r)) => UnscaledValue::U64(v),
            Self::U128((v, _r)) => UnscaledValue::U128(v),
            Self::I8((v, _r)) => UnscaledValue::I8(v),
            Self::I16((v, _r)) => UnscaledValue::I16(v),
            Self::I32((v, _r)) => UnscaledValue::I32(v),
            Self::I64((v, _r)) => UnscaledValue::I64(v),
            Self::I128((v, _r)) => UnscaledValue::I128(v),
            Self::F32((v, _r)) => UnscaledValue::F32(v),
            Self::F64((v, _r)) => UnscaledValue::F64(v),
            Self::Ascii(v) => UnscaledValue::Ascii(v),
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
        // `0x` + zero-padded two's-complement hex, width = 2 hex digits per byte.
        macro_rules! hex {
            ($v:expr, $width:expr) => {
                format!("0x{:01$X}", $v, $width)
            };
        }
        match self {
            Self::U8((v, _)) => hex!(v, 2),
            Self::U16((v, _)) => hex!(v, 4),
            Self::U32((v, _)) => hex!(v, 8),
            Self::U64((v, _)) => hex!(v, 16),
            Self::U128((v, _)) => hex!(v, 32),
            Self::I8((v, _)) => hex!(v, 2),
            Self::I16((v, _)) => hex!(v, 4),
            Self::I32((v, _)) => hex!(v, 8),
            Self::I64((v, _)) => hex!(v, 16),
            Self::I128((v, _)) => hex!(v, 32),
            Self::F32((v, _)) => hex!(v.to_bits(), 8),
            Self::F64((v, _)) => hex!(v.to_bits(), 16),
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
}

#[cfg(test)]
mod tests {
    use super::Value;
    use crate::format::Resolution;

    fn res() -> Resolution {
        Resolution(1.0)
    }

    #[test]
    fn ut_value_as_str_no_scaling() {
        assert_eq!(Value::U8((42, res())).to_string(), "42");
        assert_eq!(Value::U16((1000, res())).to_string(), "1000");
        assert_eq!(Value::I8((-1, res())).to_string(), "-1");
        assert_eq!(Value::I16((-100, res())).to_string(), "-100");
        assert_eq!(Value::Ascii("hello".to_string()).to_string(), "hello");
    }

    #[test]
    fn ut_value_as_str_with_scaling() {
        // Use resolution 2.0 so that integer * 2.0 is exact in f64
        let r = Resolution(2.0);
        assert_eq!(Value::U16((5, r.clone())).to_string(), "10");
        assert_eq!(Value::I32((-3, r.clone())).to_string(), "-6");
        assert_eq!(Value::F32((1.5f32, r.clone())).to_string(), "3");
    }

    #[test]
    fn ut_value_unscaled_drops_resolution() {
        // The unscaled string is the raw value, regardless of resolution.
        let r = Resolution(2.0);
        assert_eq!(Value::U16((5, r.clone())).unscaled().to_string(), "5");
        assert_eq!(Value::I32((-3, r.clone())).unscaled().to_string(), "-3");
        assert_eq!(
            Value::F32((1.5f32, r.clone())).unscaled().to_string(),
            "1.5"
        );
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
        assert!(!Value::U16((0, res())).is_empty());
    }

    #[test]
    fn ut_value_as_hex_str() {
        assert_eq!(Value::U8((0xFF, res())).as_hex_str(), "0xFF");
        assert_eq!(Value::U16((0x1234, res())).as_hex_str(), "0x1234");
        assert_eq!(Value::U32((0x12345678, res())).as_hex_str(), "0x12345678");
        assert_eq!(Value::U64((0, res())).as_hex_str(), "0x0000000000000000");
        // Negative i8 formatted as bit-pattern hex: -1i8 as u8 = 0xFF
        assert_eq!(Value::I8((-1i8, res())).as_hex_str(), "0xFF");
        assert_eq!(Value::I16((-1i16, res())).as_hex_str(), "0xFFFF");
        // ASCII: each byte represented as 2 hex digits
        assert_eq!(Value::Ascii("AB".to_string()).as_hex_str(), "0x4142");
    }

    #[test]
    fn ut_value_as_hex_str_f32() {
        let bits = 1.5f32.to_bits();
        let expected = format!("0x{:08X}", bits);
        assert_eq!(Value::F32((1.5f32, res())).as_hex_str(), expected);
    }

    #[test]
    fn ut_value_as_hex_str_f64() {
        let bits = 1.5f64.to_bits();
        let expected = format!("0x{:016X}", bits);
        assert_eq!(Value::F64((1.5f64, res())).as_hex_str(), expected);
    }

    #[test]
    fn ut_unscaled_value_display_all_variants() {
        use super::UnscaledValue;
        assert_eq!(UnscaledValue::U8(8).to_string(), "8");
        assert_eq!(UnscaledValue::U16(16).to_string(), "16");
        assert_eq!(UnscaledValue::U32(32).to_string(), "32");
        assert_eq!(UnscaledValue::U64(64).to_string(), "64");
        assert_eq!(UnscaledValue::U128(128).to_string(), "128");
        assert_eq!(UnscaledValue::I8(-8).to_string(), "-8");
        assert_eq!(UnscaledValue::I16(-16).to_string(), "-16");
        assert_eq!(UnscaledValue::I32(-32).to_string(), "-32");
        assert_eq!(UnscaledValue::I64(-64).to_string(), "-64");
        assert_eq!(UnscaledValue::I128(-128).to_string(), "-128");
        assert_eq!(UnscaledValue::F32(1.5).to_string(), "1.5");
        assert_eq!(UnscaledValue::F64(2.5).to_string(), "2.5");
        assert_eq!(UnscaledValue::Ascii("hi".to_string()).to_string(), "hi");
    }

    #[test]
    fn ut_value_display_all_numeric_variants() {
        // Resolution 1.0 keeps the scaled value equal to the raw value.
        assert_eq!(Value::U32((32, res())).to_string(), "32");
        assert_eq!(Value::U64((64, res())).to_string(), "64");
        assert_eq!(Value::U128((128, res())).to_string(), "128");
        assert_eq!(Value::I64((-64, res())).to_string(), "-64");
        assert_eq!(Value::I128((-128, res())).to_string(), "-128");
        assert_eq!(Value::F64((2.5, res())).to_string(), "2.5");
    }

    #[test]
    fn ut_value_unscaled_all_variants() {
        use super::UnscaledValue;
        assert!(matches!(Value::U8((8, res())).unscaled(), UnscaledValue::U8(8)));
        assert!(matches!(
            Value::U32((32, res())).unscaled(),
            UnscaledValue::U32(32)
        ));
        assert!(matches!(
            Value::U64((64, res())).unscaled(),
            UnscaledValue::U64(64)
        ));
        assert!(matches!(
            Value::U128((128, res())).unscaled(),
            UnscaledValue::U128(128)
        ));
        assert!(matches!(
            Value::I8((-8, res())).unscaled(),
            UnscaledValue::I8(-8)
        ));
        assert!(matches!(
            Value::I16((-16, res())).unscaled(),
            UnscaledValue::I16(-16)
        ));
        assert!(matches!(
            Value::I64((-64, res())).unscaled(),
            UnscaledValue::I64(-64)
        ));
        assert!(matches!(
            Value::I128((-128, res())).unscaled(),
            UnscaledValue::I128(-128)
        ));
        assert!(matches!(
            Value::F64((2.5, res())).unscaled(),
            UnscaledValue::F64(_)
        ));
    }

    #[test]
    fn ut_value_as_hex_str_remaining_variants() {
        assert_eq!(
            Value::U128((0x1, res())).as_hex_str(),
            "0x00000000000000000000000000000001"
        );
        assert_eq!(Value::I32((-1i32, res())).as_hex_str(), "0xFFFFFFFF");
        assert_eq!(
            Value::I64((-1i64, res())).as_hex_str(),
            "0xFFFFFFFFFFFFFFFF"
        );
        assert_eq!(
            Value::I128((-1i128, res())).as_hex_str(),
            "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"
        );
    }
}
