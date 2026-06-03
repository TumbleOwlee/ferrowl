pub mod enums;
pub mod format;
pub mod traits;
pub mod value;

use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use serde::{Deserialize, Serialize};
use tokio_modbus::SlaveId;

pub use crate::enums::{Access, Address, Kind};
pub use crate::format::{Alignment, Endian, Format};
pub use crate::traits::{IntoVec, ParseFromU8};
pub use crate::value::Value;

#[derive(
    Builder, Serialize, Deserialize, Debug, Clone, Getters, Setters, CopyGetters, WithSetters,
)]
#[getset(set = "pub")]
pub struct Register {
    #[getset(get = "pub")]
    #[builder(default = "0")]
    slave_id: SlaveId,
    #[getset(get = "pub")]
    #[builder(default = "Access::ReadWrite")]
    access: Access,
    #[getset(get = "pub")]
    #[builder(default = "Kind::InputRegister")]
    kind: Kind,
    #[getset(get = "pub")]
    #[builder(default = "Address::Virtual")]
    address: Address,
    #[getset(get = "pub")]
    format: Format,
}

impl Register {
    pub fn decode(&self, bytes: &[u16]) -> anyhow::Result<Value> {
        let width = self.format.width();
        if bytes.len() < width {
            Err(anyhow::anyhow!(format!(
                "Too few bytes to parse {:?}",
                self.format
            )))
        } else {
            let bytes = bytes
                .iter()
                .take(width)
                .flat_map(|v| [(v >> 8) as u8, (v & 0xFF) as u8]);

            match &self.format {
                Format::U8((e, r)) => Ok(Value::U8((
                    match e {
                        Endian::Big => ParseFromU8::<u16>::parse(bytes) as u8,
                        Endian::Little => ParseFromU8::<u16>::parse(bytes.rev()) as u8,
                    },
                    r.clone(),
                ))),
                Format::U16((e, r)) => Ok(Value::U16((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::U32((e, r)) => Ok(Value::U32((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::U64((e, r)) => Ok(Value::U64((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::U128((e, r)) => Ok(Value::U128((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::I8((e, r)) => Ok(Value::I8((
                    match e {
                        Endian::Big => ParseFromU8::<u16>::parse(bytes) as i8,
                        Endian::Little => ParseFromU8::<u16>::parse(bytes.rev()) as i8,
                    },
                    r.clone(),
                ))),
                Format::I16((e, r)) => Ok(Value::I16((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::I32((e, r)) => Ok(Value::I32((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::I64((e, r)) => Ok(Value::I64((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::I128((e, r)) => Ok(Value::I128((
                    match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    },
                    r.clone(),
                ))),
                Format::F32((e, r)) => {
                    let u: u32 = match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    };
                    Ok(Value::F32((f32::from_bits(u), r.clone())))
                }
                Format::F64((e, r)) => {
                    let u: u64 = match e {
                        Endian::Big => bytes.parse(),
                        Endian::Little => bytes.rev().parse(),
                    };
                    Ok(Value::F64((f64::from_bits(u), r.clone())))
                }
                Format::Ascii(_) => Ok(Value::Ascii(
                    String::from_utf8(bytes.collect())
                        .map_err(|_| anyhow::anyhow!("Parse PackedAscii failed."))?,
                )),
            }
        }
    }

    pub fn encode(&self, s: &str) -> anyhow::Result<Vec<u16>> {
        match &self.format {
            Format::F32((e, _)) => {
                let val: f32 = if let Some(s) = s.strip_prefix("0x") {
                    u32::from_str_radix(s, 16).map(f32::from_bits)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_bits().to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_bits().to_le_bytes().iter().into_vec()?,
                })
            }
            Format::F64((e, _)) => {
                let val: f64 = if let Some(s) = s.strip_prefix("0x") {
                    u64::from_str_radix(s, 16).map(f64::from_bits)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_bits().to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_bits().to_le_bytes().iter().into_vec()?,
                })
            }
            Format::Ascii((a, w)) => {
                let length = 2 * w.0;

                let mut zeroes = itertools::repeat_n(0, 0);
                if s.len() < length {
                    zeroes = itertools::repeat_n(0u8, length - s.len());
                }

                match a {
                    Alignment::Left => Ok(s.bytes().chain(zeroes).take(length).into_vec()?),
                    Alignment::Right => Ok(zeroes.chain(s.bytes()).take(length).into_vec()?),
                }
            }
            Format::U8((e, _)) => {
                let val: u8 = if let Some(s) = s.strip_prefix("0x") {
                    u8::from_str_radix(s, 16)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => vec![val as u16],
                    Endian::Little => vec![(val as u16) << 8],
                })
            }
            Format::U16((e, _)) => {
                let val: u16 = if let Some(s) = s.strip_prefix("0x") {
                    u16::from_str_radix(s, 16)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::U32((e, _)) => {
                let val: u32 = if let Some(s) = s.strip_prefix("0x") {
                    u32::from_str_radix(s, 16)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::U64((e, _)) => {
                let val: u64 = if let Some(s) = s.strip_prefix("0x") {
                    u64::from_str_radix(s, 16)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::U128((e, _)) => {
                let val: u128 = if let Some(s) = s.strip_prefix("0x") {
                    u128::from_str_radix(s, 16)?
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::I8((e, _)) => {
                let val: i8 = if let Some(s) = s.strip_prefix("-0x") {
                    -i8::from_str_radix(s, 16)?
                } else if let Some(s) = s.strip_prefix("0x") {
                    u8::from_str_radix(s, 16)? as i8
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => vec![val as u16],
                    Endian::Little => vec![(val as u16) << 8],
                })
            }
            Format::I16((e, _)) => {
                let val: i16 = if let Some(s) = s.strip_prefix("-0x") {
                    -i16::from_str_radix(s, 16)?
                } else if let Some(s) = s.strip_prefix("0x") {
                    u16::from_str_radix(s, 16)? as i16
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::I32((e, _)) => {
                let val: i32 = if let Some(s) = s.strip_prefix("-0x") {
                    -i32::from_str_radix(s, 16)?
                } else if let Some(s) = s.strip_prefix("0x") {
                    u32::from_str_radix(s, 16)? as i32
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::I64((e, _)) => {
                let val: i64 = if let Some(s) = s.strip_prefix("-0x") {
                    -i64::from_str_radix(s, 16)?
                } else if let Some(s) = s.strip_prefix("0x") {
                    u64::from_str_radix(s, 16)? as i64
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
            Format::I128((e, _)) => {
                let val: i128 = if let Some(s) = s.strip_prefix("-0x") {
                    -i128::from_str_radix(s, 16)?
                } else if let Some(s) = s.strip_prefix("0x") {
                    u128::from_str_radix(s, 16)? as i128
                } else {
                    s.parse()?
                };
                Ok(match e {
                    Endian::Big => val.to_be_bytes().iter().into_vec()?,
                    Endian::Little => val.to_le_bytes().iter().into_vec()?,
                })
            }
        }
    }
}

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::format::{Alignment, Endian, Resolution, Width};

        fn reg(fmt: Format) -> Register {
            RegisterBuilder::default().format(fmt).build().unwrap()
        }

        fn res() -> Resolution {
            Resolution(1.0)
        }

        // --- Format helpers ---

        fn u8_be() -> Format {
            Format::U8((Endian::Big, res()))
        }
        fn u8_le() -> Format {
            Format::U8((Endian::Little, res()))
        }
        fn u32_be() -> Format {
            Format::U32((Endian::Big, res()))
        }
        fn u32_le() -> Format {
            Format::U32((Endian::Little, res()))
        }
        fn i8_be() -> Format {
            Format::I8((Endian::Big, res()))
        }
        fn i32_be() -> Format {
            Format::I32((Endian::Big, res()))
        }
        fn i32_le() -> Format {
            Format::I32((Endian::Little, res()))
        }
        fn f32_be() -> Format {
            Format::F32((Endian::Big, res()))
        }
        fn f64_be() -> Format {
            Format::F64((Endian::Big, res()))
        }

        // --- Too few bytes ---

        #[test]
        fn ut_decode_too_few_bytes() {
            assert!(reg(u32_be()).decode(&[0x0001]).is_err());
            assert!(reg(Format::U64((Endian::Big, res()))).decode(&[]).is_err());
        }

        // --- U8 decode ---

        #[test]
        fn ut_decode_u8_big() {
            match reg(u8_be()).decode(&[0x00FF]).unwrap() {
                Value::U8((v, _)) => assert_eq!(v, 0xFF),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_decode_u8_little() {
            match reg(u8_le()).decode(&[0xFF00]).unwrap() {
                Value::U8((v, _)) => assert_eq!(v, 0xFF),
                _ => panic!("Wrong variant"),
            }
        }

        // --- U8 encode ---

        #[test]
        fn ut_encode_u8_big_decimal() {
            assert_eq!(reg(u8_be()).encode("255").unwrap(), vec![0x00FFu16]);
        }

        #[test]
        fn ut_encode_u8_big_hex() {
            assert_eq!(reg(u8_be()).encode("0xFF").unwrap(), vec![0x00FFu16]);
        }

        #[test]
        fn ut_encode_u8_little() {
            assert_eq!(reg(u8_le()).encode("255").unwrap(), vec![0xFF00u16]);
        }

        // --- U8 round-trip ---

        #[test]
        fn ut_roundtrip_u8_big() {
            let r = reg(u8_be());
            let words = r.encode("200").unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.as_str(), "200");
        }

        #[test]
        fn ut_roundtrip_u8_little() {
            let r = reg(u8_le());
            let words = r.encode("42").unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.as_str(), "42");
        }

        // --- I8 decode ---

        #[test]
        fn ut_decode_i8_negative() {
            // -1i8 as u8 = 0xFF; stored in low byte of register
            match reg(i8_be()).decode(&[0x00FF]).unwrap() {
                Value::I8((v, _)) => assert_eq!(v, -1i8),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_decode_i8_positive() {
            match reg(i8_be()).decode(&[0x0042]).unwrap() {
                Value::I8((v, _)) => assert_eq!(v, 66i8),
                _ => panic!("Wrong variant"),
            }
        }

        // --- I8 encode ---

        #[test]
        fn ut_encode_i8_decimal_negative() {
            assert_eq!(reg(i8_be()).encode("-1").unwrap(), vec![-1i8 as u16]);
        }

        #[test]
        fn ut_encode_i8_hex() {
            // "0xFF" → u8 0xFF as i8 = -1
            assert_eq!(reg(i8_be()).encode("0xFF").unwrap(), vec![-1i8 as u16]);
        }

        #[test]
        fn ut_encode_i8_neg_hex() {
            // "-0x01" → -1i8
            assert_eq!(reg(i8_be()).encode("-0x01").unwrap(), vec![-1i8 as u16]);
        }

        // --- I8 round-trip ---

        #[test]
        fn ut_roundtrip_i8() {
            let r = reg(i8_be());
            for val in [-128i8, -1, 0, 1, 127] {
                let words = r.encode(&val.to_string()).unwrap();
                let decoded = r.decode(&words).unwrap();
                assert_eq!(decoded.as_str(), val.to_string());
            }
        }

        // --- U32 decode ---

        #[test]
        fn ut_decode_u32_big() {
            match reg(u32_be()).decode(&[0x0001, 0x0002]).unwrap() {
                Value::U32((v, _)) => assert_eq!(v, 0x00010002),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_decode_u32_little() {
            // Bytes [0x01, 0x02, 0x03, 0x04] reversed = [0x04, 0x03, 0x02, 0x01]
            // parse = 0x04030201
            match reg(u32_le()).decode(&[0x0102, 0x0304]).unwrap() {
                Value::U32((v, _)) => assert_eq!(v, 0x04030201),
                _ => panic!("Wrong variant"),
            }
        }

        // --- U32 encode ---

        #[test]
        fn ut_encode_u32_big() {
            // 65538 = 0x00010002
            assert_eq!(reg(u32_be()).encode("65538").unwrap(), vec![0x0001u16, 0x0002u16]);
        }

        #[test]
        fn ut_encode_u32_big_hex() {
            assert_eq!(
                reg(u32_be()).encode("0x00010002").unwrap(),
                vec![0x0001u16, 0x0002u16]
            );
        }

        #[test]
        fn ut_encode_u32_little() {
            // 0x00010002 in LE bytes: [0x02, 0x00, 0x01, 0x00] → registers [0x0200, 0x0100]
            assert_eq!(
                reg(u32_le()).encode("65538").unwrap(),
                vec![0x0200u16, 0x0100u16]
            );
        }

        // --- U32 round-trip ---

        #[test]
        fn ut_roundtrip_u32_big() {
            let r = reg(u32_be());
            let words = r.encode("123456789").unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.as_str(), "123456789");
        }

        #[test]
        fn ut_roundtrip_u32_little() {
            let r = reg(u32_le());
            let words = r.encode("987654321").unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.as_str(), "987654321");
        }

        // --- I32 round-trip ---

        #[test]
        fn ut_roundtrip_i32_big() {
            let r = reg(i32_be());
            for val in [-2147483648i32, -1, 0, 1, 2147483647] {
                let words = r.encode(&val.to_string()).unwrap();
                let decoded = r.decode(&words).unwrap();
                assert_eq!(decoded.as_str(), val.to_string(), "val={}", val);
            }
        }

        #[test]
        fn ut_encode_i32_neg_hex() {
            // "-0x01" → -1i32
            let r = reg(i32_be());
            let words = r.encode("-0x01").unwrap();
            match r.decode(&words).unwrap() {
                Value::I32((v, _)) => assert_eq!(v, -1),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_encode_i32_hex_two_complement() {
            // "0xFFFFFFFF" → u32 all-ones as i32 = -1
            let r = reg(i32_be());
            let words = r.encode("0xFFFFFFFF").unwrap();
            match r.decode(&words).unwrap() {
                Value::I32((v, _)) => assert_eq!(v, -1),
                _ => panic!("Wrong variant"),
            }
        }

        // --- F32 ---

        #[test]
        fn ut_decode_f32_big() {
            let bits = 1.5f32.to_bits();
            let words = vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16];
            match reg(f32_be()).decode(&words).unwrap() {
                Value::F32((f, _)) => assert!((f - 1.5f32).abs() < 1e-6),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_encode_f32_decimal() {
            let bits = 1.5f32.to_bits();
            let expected = vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16];
            assert_eq!(reg(f32_be()).encode("1.5").unwrap(), expected);
        }

        #[test]
        fn ut_roundtrip_f32_big() {
            let r = reg(f32_be());
            let words = r.encode("1.5").unwrap();
            match r.decode(&words).unwrap() {
                Value::F32((f, _)) => assert!((f - 1.5f32).abs() < 1e-6),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_encode_f32_hex() {
            let bits = 1.5f32.to_bits();
            let hex_str = format!("0x{:08X}", bits);
            let expected = vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16];
            assert_eq!(reg(f32_be()).encode(&hex_str).unwrap(), expected);
        }

        // --- F64 ---

        #[test]
        fn ut_roundtrip_f64_big() {
            let r = reg(f64_be());
            let words = r.encode("1.5").unwrap();
            match r.decode(&words).unwrap() {
                Value::F64((f, _)) => assert!((f - 1.5f64).abs() < 1e-10),
                _ => panic!("Wrong variant"),
            }
        }

        // --- Ascii ---

        #[test]
        fn ut_decode_ascii_exact_fill() {
            // "ABCD" fills exactly 4 bytes (Width(2) = 2 registers = 4 bytes)
            let r = reg(Format::Ascii((Alignment::Left, Width(2))));
            match r.decode(&[0x4142, 0x4344]).unwrap() {
                Value::Ascii(s) => assert_eq!(s, "ABCD"),
                _ => panic!("Wrong variant"),
            }
        }

        #[test]
        fn ut_encode_ascii_left_aligned() {
            // "AB" left-aligned in 4 bytes: [0x41, 0x42, 0x00, 0x00]
            let r = reg(Format::Ascii((Alignment::Left, Width(2))));
            assert_eq!(r.encode("AB").unwrap(), vec![0x4142u16, 0x0000u16]);
        }

        #[test]
        fn ut_encode_ascii_right_aligned() {
            // "AB" right-aligned in 4 bytes: [0x00, 0x00, 0x41, 0x42]
            let r = reg(Format::Ascii((Alignment::Right, Width(2))));
            assert_eq!(r.encode("AB").unwrap(), vec![0x0000u16, 0x4142u16]);
        }

        #[test]
        fn ut_roundtrip_ascii_exact() {
            // Exact fill avoids null-padding in round-trip
            let r = reg(Format::Ascii((Alignment::Left, Width(2))));
            let words = r.encode("ABCD").unwrap();
            match r.decode(&words).unwrap() {
                Value::Ascii(s) => assert_eq!(s, "ABCD"),
                _ => panic!("Wrong variant"),
            }
        }

        // --- Resolution scaling ---

        #[test]
        fn ut_decode_u16_with_resolution() {
            let r = reg(Format::U16((Endian::Big, Resolution(0.5))));
            let words = r.encode("2048").unwrap();
            let decoded = r.decode(&words).unwrap();
            // 2048 * 0.5 = 1024.0
            assert_eq!(decoded.as_str(), "1024");
        }
    }
