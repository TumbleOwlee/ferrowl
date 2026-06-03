use serde::{Deserialize, Serialize};

use crate::format::Resolution;

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
        write!(fmt, "{}", self.as_str())
    }
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Self::U8((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::U16((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::U32((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::U64((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::U128((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::I8((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::I16((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::I32((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::I64((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::I128((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::F32((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::F64((v, r)) => {
                let v = *v as f64 * r.0;
                format!("{v}")
            }
            Self::Ascii(v) => v.to_string(),
        }
    }

    pub fn as_hex_str(&self) -> String {
        match self {
            Self::U8((v, _)) => format!("0x{:01$X}", v, 2),
            Self::U16((v, _)) => format!("0x{:01$X}", v, 4),
            Self::U32((v, _)) => format!("0x{:01$X}", v, 8),
            Self::U64((v, _)) => format!("0x{:01$X}", v, 16),
            Self::U128((v, _)) => format!("0x{:01$X}", v, 32),
            Self::I8((v, _)) => format!("0x{:01$X}", v, 2),
            Self::I16((v, _)) => format!("0x{:01$X}", v, 4),
            Self::I32((v, _)) => format!("0x{:01$X}", v, 8),
            Self::I64((v, _)) => format!("0x{:01$X}", v, 16),
            Self::I128((v, _)) => format!("0x{:01$X}", v, 32),
            Self::F32((v, _)) => format!("0x{:01$X}", v.to_bits(), 8),
            Self::F64((v, _)) => format!("0x{:01$X}", v.to_bits(), 16),
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
        assert_eq!(Value::U8((42, res())).as_str(), "42");
        assert_eq!(Value::U16((1000, res())).as_str(), "1000");
        assert_eq!(Value::I8((-1, res())).as_str(), "-1");
        assert_eq!(Value::I16((-100, res())).as_str(), "-100");
        assert_eq!(Value::Ascii("hello".to_string()).as_str(), "hello");
    }

    #[test]
    fn ut_value_as_str_with_scaling() {
        // Use resolution 2.0 so that integer * 2.0 is exact in f64
        let r = Resolution(2.0);
        assert_eq!(Value::U16((5, r.clone())).as_str(), "10");
        assert_eq!(Value::I32((-3, r.clone())).as_str(), "-6");
        assert_eq!(Value::F32((1.5f32, r.clone())).as_str(), "3");
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
}
