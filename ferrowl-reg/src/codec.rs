//! Decode register words into [`Value`]s and encode string input back into words.

use crate::format::{Alignment, Endian, Format};
use crate::traits::{IntoVec, ParseFromU8};
use crate::value::Value;

/// Decodes raw register words into a typed [`Value`] according to `format`.
///
/// Only the first `format.width()` words are consumed; errors if `bytes` is
/// shorter than that, or if ASCII data is not valid UTF-8. For integer formats
/// the configured [`BitField`] is applied: `field = (raw & mask) >> shift`.
pub fn decode(format: &Format, bytes: &[u16]) -> anyhow::Result<Value> {
    let width = format.width();
    if bytes.len() < width {
        Err(anyhow::anyhow!(format!(
            "Too few bytes to parse {:?}",
            format
        )))
    } else {
        let bytes = bytes
            .iter()
            .take(width)
            .flat_map(|v| [(v >> 8) as u8, (v & 0xFF) as u8]);

        // Big-endian parses the byte stream as-is; little-endian reverses it
        // first. The raw word is taken in the unsigned domain (`$uty`) so the
        // mask/shift act on the bit pattern, then cast to the target type.
        macro_rules! decode_int {
            ($variant:ident, $uty:ty, $ty:ty, $e:expr, $r:expr, $bf:expr) => {{
                let raw: $uty = match $e {
                    Endian::Big => bytes.parse(),
                    Endian::Little => bytes.rev().parse(),
                };
                let field = (((raw as u128) & $bf.mask) >> $bf.shift()) as $ty;
                Ok(Value::$variant((field, $r.clone())))
            }};
        }
        // U8/I8 occupy a single register, so parse a u16 then narrow.
        macro_rules! decode_byte {
            ($variant:ident, $ty:ty, $e:expr, $r:expr, $bf:expr) => {{
                let raw: u16 = match $e {
                    Endian::Big => ParseFromU8::<u16>::parse(bytes),
                    Endian::Little => ParseFromU8::<u16>::parse(bytes.rev()),
                };
                let field = (((raw as u128) & $bf.mask) >> $bf.shift()) as $ty;
                Ok(Value::$variant((field, $r.clone())))
            }};
        }
        match format {
            Format::U8((e, r, bf)) => decode_byte!(U8, u8, e, r, bf),
            Format::I8((e, r, bf)) => decode_byte!(I8, i8, e, r, bf),
            Format::U16((e, r, bf)) => decode_int!(U16, u16, u16, e, r, bf),
            Format::U32((e, r, bf)) => decode_int!(U32, u32, u32, e, r, bf),
            Format::U64((e, r, bf)) => decode_int!(U64, u64, u64, e, r, bf),
            Format::U128((e, r, bf)) => decode_int!(U128, u128, u128, e, r, bf),
            Format::I16((e, r, bf)) => decode_int!(I16, u16, i16, e, r, bf),
            Format::I32((e, r, bf)) => decode_int!(I32, u32, i32, e, r, bf),
            Format::I64((e, r, bf)) => decode_int!(I64, u64, i64, e, r, bf),
            Format::I128((e, r, bf)) => decode_int!(I128, u128, i128, e, r, bf),
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

/// Parses a user-entered string into raw register words according to `format`.
///
/// Numeric input accepts decimal or `0x`-prefixed hex (`-0x` for negative
/// signed values; a plain `0x` literal on a signed/float format is taken as
/// the bit pattern). ASCII input is zero-padded or truncated to the format
/// width according to its alignment. Note that resolution is *not* applied —
/// the string is the raw value. For an integer format the value is placed
/// according to the [`BitField`] (`raw = (value << shift) & mask`) with all
/// other bits left zero.
pub fn encode(format: &Format, s: &str) -> anyhow::Result<Vec<u16>> {
    // Multi-byte unsigned: parse decimal or `0x` hex, position per the bit-field,
    // then split to register words.
    macro_rules! encode_uint {
        ($ty:ty, $e:expr, $s:expr, $bf:expr) => {{
            let val: $ty = if let Some(s) = $s.strip_prefix("0x") {
                <$ty>::from_str_radix(s, 16)?
            } else {
                $s.parse()?
            };
            let val = ((((val as u128) << $bf.shift()) & $bf.mask)) as $ty;
            Ok(match $e {
                Endian::Big => val.to_be_bytes().iter().into_vec()?,
                Endian::Little => val.to_le_bytes().iter().into_vec()?,
            })
        }};
    }
    // Multi-byte signed: also accept `-0x` hex; `$uty` is the same-width unsigned type
    // used both to reinterpret a `0x` literal as a bit pattern and to apply the
    // bit-field in the unsigned domain.
    macro_rules! encode_int {
        ($ty:ty, $uty:ty, $e:expr, $s:expr, $bf:expr) => {{
            let val: $ty = if let Some(s) = $s.strip_prefix("-0x") {
                -<$ty>::from_str_radix(s, 16)?
            } else if let Some(s) = $s.strip_prefix("0x") {
                <$uty>::from_str_radix(s, 16)? as $ty
            } else {
                $s.parse()?
            };
            let val =
                (((((val as $uty) as u128) << $bf.shift()) & $bf.mask) as $uty) as $ty;
            Ok(match $e {
                Endian::Big => val.to_be_bytes().iter().into_vec()?,
                Endian::Little => val.to_le_bytes().iter().into_vec()?,
            })
        }};
    }
    match format {
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
        Format::U8((e, _, bf)) => {
            let val: u8 = if let Some(s) = s.strip_prefix("0x") {
                u8::from_str_radix(s, 16)?
            } else {
                s.parse()?
            };
            let val = ((((val as u128) << bf.shift()) & bf.mask)) as u8;
            Ok(match e {
                Endian::Big => vec![val as u16],
                Endian::Little => vec![(val as u16) << 8],
            })
        }
        Format::U16((e, _, bf)) => encode_uint!(u16, e, s, bf),
        Format::U32((e, _, bf)) => encode_uint!(u32, e, s, bf),
        Format::U64((e, _, bf)) => encode_uint!(u64, e, s, bf),
        Format::U128((e, _, bf)) => encode_uint!(u128, e, s, bf),
        Format::I8((e, _, bf)) => {
            let val: i8 = if let Some(s) = s.strip_prefix("-0x") {
                -i8::from_str_radix(s, 16)?
            } else if let Some(s) = s.strip_prefix("0x") {
                u8::from_str_radix(s, 16)? as i8
            } else {
                s.parse()?
            };
            let val = (((((val as u8) as u128) << bf.shift()) & bf.mask) as u8) as i8;
            Ok(match e {
                Endian::Big => vec![val as u16],
                Endian::Little => vec![(val as u16) << 8],
            })
        }
        Format::I16((e, _, bf)) => encode_int!(i16, u16, e, s, bf),
        Format::I32((e, _, bf)) => encode_int!(i32, u32, e, s, bf),
        Format::I64((e, _, bf)) => encode_int!(i64, u64, e, s, bf),
        Format::I128((e, _, bf)) => encode_int!(i128, u128, e, s, bf),
    }
}

/// Per-word mask selecting the bits an integer [`BitField`] format owns, laid
/// out in the same endian order as [`encode`]. Full-width integers (and float/
/// ASCII formats) yield all-`0xFFFF` words, so a read-modify-write merge against
/// them overwrites the whole value as before.
pub fn mask_words(format: &Format) -> Vec<u16> {
    macro_rules! mask_uint {
        ($ty:ty, $e:expr, $bf:expr) => {{
            let m = $bf.mask as $ty;
            match $e {
                Endian::Big => m.to_be_bytes().iter().into_vec().unwrap_or_default(),
                Endian::Little => m.to_le_bytes().iter().into_vec().unwrap_or_default(),
            }
        }};
    }
    match format {
        Format::U8((e, _, bf)) | Format::I8((e, _, bf)) => {
            let m = bf.mask as u8;
            match e {
                Endian::Big => vec![m as u16],
                Endian::Little => vec![(m as u16) << 8],
            }
        }
        Format::U16((e, _, bf)) | Format::I16((e, _, bf)) => mask_uint!(u16, e, bf),
        Format::U32((e, _, bf)) | Format::I32((e, _, bf)) => mask_uint!(u32, e, bf),
        Format::U64((e, _, bf)) | Format::I64((e, _, bf)) => mask_uint!(u64, e, bf),
        Format::U128((e, _, bf)) | Format::I128((e, _, bf)) => mask_uint!(u128, e, bf),
        Format::F32(_) | Format::F64(_) | Format::Ascii(_) => vec![0xFFFFu16; format.width()],
    }
}

#[cfg(test)]
mod tests {
    use crate::format::{Alignment, BitField, Endian, Format, Resolution, Width};
    use crate::value::Value;
    use crate::{Register, RegisterBuilder};

    fn reg(fmt: Format) -> Register {
        RegisterBuilder::default().format(fmt).build().unwrap()
    }

    fn res() -> Resolution {
        Resolution(1.0)
    }

    fn bf() -> BitField {
        BitField::default()
    }

    // --- Format helpers ---

    fn u8_be() -> Format {
        Format::U8((Endian::Big, res(), bf()))
    }
    fn u8_le() -> Format {
        Format::U8((Endian::Little, res(), bf()))
    }
    fn u32_be() -> Format {
        Format::U32((Endian::Big, res(), bf()))
    }
    fn u32_le() -> Format {
        Format::U32((Endian::Little, res(), bf()))
    }
    fn i8_be() -> Format {
        Format::I8((Endian::Big, res(), bf()))
    }
    fn i32_be() -> Format {
        Format::I32((Endian::Big, res(), bf()))
    }
    fn i32_le() -> Format {
        Format::I32((Endian::Little, res(), bf()))
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
        assert!(
            reg(Format::U64((Endian::Big, res(), bf())))
                .decode(&[])
                .is_err()
        );
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
        assert_eq!(
            reg(u32_be()).encode("65538").unwrap(),
            vec![0x0001u16, 0x0002u16]
        );
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
    fn ut_roundtrip_i32_little() {
        let r = reg(i32_le());
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
        let r = reg(Format::U16((Endian::Big, Resolution(0.5), bf())));
        let words = r.encode("2048").unwrap();
        let decoded = r.decode(&words).unwrap();
        // 2048 * 0.5 = 1024.0
        assert_eq!(decoded.as_str(), "1024");
    }

    // --- Bit-field mask + derived shift ---

    fn u16_be_mask(mask: u128) -> Format {
        Format::U16((Endian::Big, res(), BitField { mask }))
    }

    #[test]
    fn ut_decode_u16_high_byte_field() {
        // mask 0xFF00 → shift 8: raw 0xAB12 reads as 0xAB.
        match reg(u16_be_mask(0xFF00)).decode(&[0xAB12]).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0xAB),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn ut_decode_u16_low_byte_field() {
        // mask 0x00FF → shift 0: raw 0xAB12 reads as 0x12.
        match reg(u16_be_mask(0x00FF)).decode(&[0xAB12]).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0x12),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn ut_encode_u16_high_byte_field() {
        // value 0x12 placed into mask 0xFF00 → word 0x1200, other bits zero.
        assert_eq!(reg(u16_be_mask(0xFF00)).encode("0x12").unwrap(), vec![0x1200u16]);
    }

    #[test]
    fn ut_roundtrip_u16_field() {
        let r = reg(u16_be_mask(0x0FF0));
        let words = r.encode("0xAB").unwrap();
        // 0xAB << 4 & 0x0FF0 = 0x0AB0
        assert_eq!(words, vec![0x0AB0u16]);
        match r.decode(&words).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0xAB),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn ut_full_mask_is_noop() {
        // Default (full) mask encodes/decodes exactly like before.
        let r = reg(u16_be_mask(u128::MAX));
        assert_eq!(r.encode("0xABCD").unwrap(), vec![0xABCDu16]);
        match r.decode(&[0xABCD]).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0xABCD),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn ut_mask_words_layout() {
        // U16 mask laid out as a single word.
        assert_eq!(reg(u16_be_mask(0xFF00)).write_mask(), vec![0xFF00u16]);
        // Full U16 mask narrows to 0xFFFF.
        assert_eq!(reg(u16_be_mask(u128::MAX)).write_mask(), vec![0xFFFFu16]);
        // U32 big-endian mask spans two words.
        let r = reg(Format::U32((Endian::Big, res(), BitField { mask: 0xFFFF_0000 })));
        assert_eq!(r.write_mask(), vec![0xFFFFu16, 0x0000u16]);
        // U8 full mask only owns the low byte of its word.
        assert_eq!(reg(u8_be()).write_mask(), vec![0x00FFu16]);
    }

    #[test]
    fn ut_merge_write_preserves_sibling_bits() {
        // Two U16 regs aliasing one address: low byte and high byte.
        let low = reg(u16_be_mask(0x00FF));
        let high = reg(u16_be_mask(0xFF00));
        // Start empty, write low = 0x12 → 0x0012.
        let mem = low.merge_write(&[0x0000], &low.encode("0x12").unwrap());
        assert_eq!(mem, vec![0x0012u16]);
        // Write high = 0x34 into the same word → 0x3412, low byte preserved.
        let mem = high.merge_write(&mem, &high.encode("0x34").unwrap());
        assert_eq!(mem, vec![0x3412u16]);
        // Both fields decode back independently.
        match low.decode(&mem).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0x12),
            _ => panic!("Wrong variant"),
        }
        match high.decode(&mem).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0x34),
            _ => panic!("Wrong variant"),
        }
    }
}
