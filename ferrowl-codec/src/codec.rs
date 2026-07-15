//! Decode register words into [`Value`]s and encode string input back into words.

use crate::error::CodecError;
use crate::format::{Alignment, Endian, Format};
use crate::traits::{IntoVec, ParseFromU8};
use crate::value::Value;

/// Decodes raw register words into a typed [`Value`] according to `format`.
///
/// Only the first `format.width()` words are consumed; errors if `bytes` is
/// shorter than that, or if ASCII data is not valid UTF-8. For integer formats
/// the configured [`BitField`] is applied: `field = (raw & mask) >> shift`.
pub fn decode(format: &Format, bytes: &[u16]) -> Result<Value, CodecError> {
    let width = format.width();
    if bytes.len() < width {
        Err(CodecError::TooFewBytes(format.clone()))
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
        macro_rules! check_width {
            ($bf:expr, $bits:expr) => {
                if !$bf.fits($bits) {
                    return Err(CodecError::BitFieldWidth(format.clone()));
                }
            };
        }
        match format {
            Format::U8((e, r, bf)) => {
                check_width!(bf, 8);
                decode_byte!(U8, u8, e, r, bf)
            }
            Format::I8((e, r, bf)) => {
                check_width!(bf, 8);
                decode_byte!(I8, i8, e, r, bf)
            }
            Format::U16((e, r, bf)) => {
                check_width!(bf, 16);
                decode_int!(U16, u16, u16, e, r, bf)
            }
            Format::U32((e, r, bf)) => {
                check_width!(bf, 32);
                decode_int!(U32, u32, u32, e, r, bf)
            }
            Format::U64((e, r, bf)) => {
                check_width!(bf, 64);
                decode_int!(U64, u64, u64, e, r, bf)
            }
            Format::U128((e, r, bf)) => {
                check_width!(bf, 128);
                decode_int!(U128, u128, u128, e, r, bf)
            }
            Format::I16((e, r, bf)) => {
                check_width!(bf, 16);
                decode_int!(I16, u16, i16, e, r, bf)
            }
            Format::I32((e, r, bf)) => {
                check_width!(bf, 32);
                decode_int!(I32, u32, i32, e, r, bf)
            }
            Format::I64((e, r, bf)) => {
                check_width!(bf, 64);
                decode_int!(I64, u64, i64, e, r, bf)
            }
            Format::I128((e, r, bf)) => {
                check_width!(bf, 128);
                decode_int!(I128, u128, i128, e, r, bf)
            }
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
                String::from_utf8(bytes.collect()).map_err(|_| CodecError::PackedAscii)?,
            )),
        }
    }
}

/// Parses a user-entered string into the logical [`Value`] `format` describes.
///
/// Numeric input accepts decimal or `0x`-prefixed hex (`-0x` for negative
/// signed values; a plain `0x` literal on a signed/float format is taken as
/// the bit pattern). ASCII input is stored verbatim (padding/truncation is
/// applied by [`encode_value`]). The parsed value is the logical field —
/// resolution and bit-field placement are applied later by [`encode_value`],
/// mirroring the value [`decode`] would hand back.
fn parse_value(format: &Format, s: &str) -> Result<Value, CodecError> {
    // Multi-byte unsigned: parse decimal or `0x` hex.
    macro_rules! parse_uint {
        ($variant:ident, $ty:ty, $r:expr, $s:expr) => {{
            let val: $ty = if let Some(s) = $s.strip_prefix("0x") {
                <$ty>::from_str_radix(s, 16)?
            } else {
                $s.parse()?
            };
            Ok(Value::$variant((val, $r.clone())))
        }};
    }
    // Multi-byte signed: also accept `-0x` hex; `$uty` is the same-width unsigned type
    // used to reinterpret a `0x` literal as a bit pattern.
    macro_rules! parse_int {
        ($variant:ident, $ty:ty, $uty:ty, $r:expr, $s:expr) => {{
            let val: $ty = if let Some(s) = $s.strip_prefix("-0x") {
                (<$uty>::from_str_radix(s, 16)? as $ty).wrapping_neg()
            } else if let Some(s) = $s.strip_prefix("0x") {
                <$uty>::from_str_radix(s, 16)? as $ty
            } else {
                $s.parse()?
            };
            Ok(Value::$variant((val, $r.clone())))
        }};
    }
    match format {
        Format::F32((_, r)) => {
            let val: f32 = if let Some(s) = s.strip_prefix("0x") {
                u32::from_str_radix(s, 16).map(f32::from_bits)?
            } else {
                s.parse()?
            };
            Ok(Value::F32((val, r.clone())))
        }
        Format::F64((_, r)) => {
            let val: f64 = if let Some(s) = s.strip_prefix("0x") {
                u64::from_str_radix(s, 16).map(f64::from_bits)?
            } else {
                s.parse()?
            };
            Ok(Value::F64((val, r.clone())))
        }
        Format::Ascii(_) => Ok(Value::Ascii(s.to_string())),
        Format::U8((_, r, bf)) => {
            if !bf.fits(8) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            let val: u8 = if let Some(s) = s.strip_prefix("0x") {
                u8::from_str_radix(s, 16)?
            } else {
                s.parse()?
            };
            Ok(Value::U8((val, r.clone())))
        }
        Format::U16((_, r, bf)) => {
            if !bf.fits(16) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_uint!(U16, u16, r, s)
        }
        Format::U32((_, r, bf)) => {
            if !bf.fits(32) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_uint!(U32, u32, r, s)
        }
        Format::U64((_, r, bf)) => {
            if !bf.fits(64) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_uint!(U64, u64, r, s)
        }
        Format::U128((_, r, bf)) => {
            if !bf.fits(128) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_uint!(U128, u128, r, s)
        }
        Format::I8((_, r, bf)) => {
            if !bf.fits(8) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            let val: i8 = if let Some(s) = s.strip_prefix("-0x") {
                (u8::from_str_radix(s, 16)? as i8).wrapping_neg()
            } else if let Some(s) = s.strip_prefix("0x") {
                u8::from_str_radix(s, 16)? as i8
            } else {
                s.parse()?
            };
            Ok(Value::I8((val, r.clone())))
        }
        Format::I16((_, r, bf)) => {
            if !bf.fits(16) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_int!(I16, i16, u16, r, s)
        }
        Format::I32((_, r, bf)) => {
            if !bf.fits(32) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_int!(I32, i32, u32, r, s)
        }
        Format::I64((_, r, bf)) => {
            if !bf.fits(64) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_int!(I64, i64, u64, r, s)
        }
        Format::I128((_, r, bf)) => {
            if !bf.fits(128) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            parse_int!(I128, i128, u128, r, s)
        }
    }
}

/// Parses a user-entered string into raw register words according to `format`.
///
/// Parses the string into the logical [`Value`] the format describes, then
/// encodes it with [`encode_value`]. See [`parse_value`] for the accepted
/// numeric/ASCII input forms.
pub fn encode(format: &Format, s: &str) -> Result<Vec<u16>, CodecError> {
    encode_value(format, &parse_value(format, s)?)
}

/// Encodes a typed logical [`Value`] into raw register words according to
/// `format`. Errors with [`CodecError::ValueFormatMismatch`] if `value`'s
/// variant does not match `format`'s. For an integer format the value is
/// placed according to the [`BitField`] (`raw = (value << shift) & mask`)
/// with all other bits left zero; resolution is *not* applied — `value` is
/// the raw field, as returned by [`decode`].
pub fn encode_value(format: &Format, value: &Value) -> Result<Vec<u16>, CodecError> {
    let mismatch = || CodecError::ValueFormatMismatch(format.clone());
    // Multi-byte unsigned: position the field per the bit-field, then split to register words.
    macro_rules! encode_uint {
        ($variant:ident, $ty:ty, $e:expr, $bf:expr) => {{
            match value {
                Value::$variant((val, _)) => {
                    let val = (((*val as u128) << $bf.shift()) & $bf.mask) as $ty;
                    Ok(match $e {
                        Endian::Big => val.to_be_bytes().iter().into_vec()?,
                        Endian::Little => val.to_le_bytes().iter().into_vec()?,
                    })
                }
                _ => Err(mismatch()),
            }
        }};
    }
    // Multi-byte signed: `$uty` is the same-width unsigned type used to apply the
    // bit-field in the unsigned domain.
    macro_rules! encode_int {
        ($variant:ident, $ty:ty, $uty:ty, $e:expr, $bf:expr) => {{
            match value {
                Value::$variant((val, _)) => {
                    let val =
                        (((((*val as $uty) as u128) << $bf.shift()) & $bf.mask) as $uty) as $ty;
                    Ok(match $e {
                        Endian::Big => val.to_be_bytes().iter().into_vec()?,
                        Endian::Little => val.to_le_bytes().iter().into_vec()?,
                    })
                }
                _ => Err(mismatch()),
            }
        }};
    }
    match format {
        Format::F32((e, _)) => match value {
            Value::F32((val, _)) => Ok(match e {
                Endian::Big => val.to_bits().to_be_bytes().iter().into_vec()?,
                Endian::Little => val.to_bits().to_le_bytes().iter().into_vec()?,
            }),
            _ => Err(mismatch()),
        },
        Format::F64((e, _)) => match value {
            Value::F64((val, _)) => Ok(match e {
                Endian::Big => val.to_bits().to_be_bytes().iter().into_vec()?,
                Endian::Little => val.to_bits().to_le_bytes().iter().into_vec()?,
            }),
            _ => Err(mismatch()),
        },
        Format::Ascii((a, w)) => match value {
            Value::Ascii(s) => {
                let length = 2 * w.0;
                let zeroes = length.saturating_sub(s.len());

                match a {
                    Alignment::Left => Ok(s
                        .bytes()
                        .chain(itertools::repeat_n(0u8, zeroes))
                        .take(length)
                        .into_vec()?),
                    // Oversized input keeps the *last* `length` bytes, not the first.
                    Alignment::Right => Ok(itertools::repeat_n(0u8, zeroes)
                        .chain(s.bytes().skip(s.len().saturating_sub(length)))
                        .take(length)
                        .into_vec()?),
                }
            }
            _ => Err(mismatch()),
        },
        Format::U8((e, _, bf)) => {
            if !bf.fits(8) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            match value {
                Value::U8((val, _)) => {
                    let val = (((*val as u128) << bf.shift()) & bf.mask) as u8;
                    Ok(match e {
                        Endian::Big => vec![val as u16],
                        Endian::Little => vec![(val as u16) << 8],
                    })
                }
                _ => Err(mismatch()),
            }
        }
        Format::U16((e, _, bf)) => {
            if !bf.fits(16) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_uint!(U16, u16, e, bf)
        }
        Format::U32((e, _, bf)) => {
            if !bf.fits(32) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_uint!(U32, u32, e, bf)
        }
        Format::U64((e, _, bf)) => {
            if !bf.fits(64) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_uint!(U64, u64, e, bf)
        }
        Format::U128((e, _, bf)) => {
            if !bf.fits(128) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_uint!(U128, u128, e, bf)
        }
        Format::I8((e, _, bf)) => {
            if !bf.fits(8) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            match value {
                Value::I8((val, _)) => {
                    let val = (((((*val as u8) as u128) << bf.shift()) & bf.mask) as u8) as i8;
                    Ok(match e {
                        Endian::Big => vec![val as u16],
                        Endian::Little => vec![(val as u16) << 8],
                    })
                }
                _ => Err(mismatch()),
            }
        }
        Format::I16((e, _, bf)) => {
            if !bf.fits(16) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_int!(I16, i16, u16, e, bf)
        }
        Format::I32((e, _, bf)) => {
            if !bf.fits(32) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_int!(I32, i32, u32, e, bf)
        }
        Format::I64((e, _, bf)) => {
            if !bf.fits(64) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_int!(I64, i64, u64, e, bf)
        }
        Format::I128((e, _, bf)) => {
            if !bf.fits(128) {
                return Err(CodecError::BitFieldWidth(format.clone()));
            }
            encode_int!(I128, i128, u128, e, bf)
        }
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
    use crate::codec::{decode, encode, encode_value};
    use crate::format::{Alignment, BitField, Endian, Format, Resolution, Width};
    use crate::value::Value;
    use crate::{CodecError, Register, RegisterBuilder};

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

    /// MB-R-023 — decoding fails with a too-few-bytes error when fewer words than the format width are supplied.
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

    /// MB-R-012 — a big-endian U8 reads the byte from the register's low byte.
    #[test]
    fn ut_decode_u8_big() {
        match reg(u8_be()).decode(&[0x00FF]).unwrap() {
            Value::U8((v, _)) => assert_eq!(v, 0xFF),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-012 — a little-endian U8 reads the byte from the register's high byte.
    #[test]
    fn ut_decode_u8_little() {
        match reg(u8_le()).decode(&[0xFF00]).unwrap() {
            Value::U8((v, _)) => assert_eq!(v, 0xFF),
            _ => panic!("Wrong variant"),
        }
    }

    // --- U8 encode ---

    /// MB-R-012 — a big-endian U8 places the byte in the register's low byte (decimal input).
    #[test]
    fn ut_encode_u8_big_decimal() {
        assert_eq!(reg(u8_be()).encode("255").unwrap(), vec![0x00FFu16]);
    }

    /// MB-R-012 — a big-endian U8 places the byte in the register's low byte (hex input).
    #[test]
    fn ut_encode_u8_big_hex() {
        assert_eq!(reg(u8_be()).encode("0xFF").unwrap(), vec![0x00FFu16]);
    }

    /// MB-R-012 — a little-endian U8 places the byte in the register's high byte.
    #[test]
    fn ut_encode_u8_little() {
        assert_eq!(reg(u8_le()).encode("255").unwrap(), vec![0xFF00u16]);
    }

    // --- U8 round-trip ---

    /// MB-R-007 — encoding then decoding a U8 recovers the value.
    #[test]
    fn ut_roundtrip_u8_big() {
        let r = reg(u8_be());
        let words = r.encode("200").unwrap();
        let decoded = r.decode(&words).unwrap();
        assert_eq!(decoded.to_string(), "200");
    }

    /// MB-R-007 — encoding then decoding a little-endian U8 recovers the value.
    #[test]
    fn ut_roundtrip_u8_little() {
        let r = reg(u8_le());
        let words = r.encode("42").unwrap();
        let decoded = r.decode(&words).unwrap();
        assert_eq!(decoded.to_string(), "42");
    }

    // --- I8 decode ---

    /// MB-R-012 — a big-endian I8 reads the byte from the register's low byte (negative value).
    #[test]
    fn ut_decode_i8_negative() {
        // -1i8 as u8 = 0xFF; stored in low byte of register
        match reg(i8_be()).decode(&[0x00FF]).unwrap() {
            Value::I8((v, _)) => assert_eq!(v, -1i8),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-012 — a big-endian I8 reads the byte from the register's low byte (positive value).
    #[test]
    fn ut_decode_i8_positive() {
        match reg(i8_be()).decode(&[0x0042]).unwrap() {
            Value::I8((v, _)) => assert_eq!(v, 66i8),
            _ => panic!("Wrong variant"),
        }
    }

    // --- I8 encode ---

    /// MB-R-022 — a signed format accepts a plain negative decimal literal.
    #[test]
    fn ut_encode_i8_decimal_negative() {
        assert_eq!(reg(i8_be()).encode("-1").unwrap(), vec![-1i8 as u16]);
    }

    /// MB-R-022 — a plain `0x` literal on a signed format is taken as the bit pattern.
    #[test]
    fn ut_encode_i8_hex() {
        // "0xFF" → u8 0xFF as i8 = -1
        assert_eq!(reg(i8_be()).encode("0xFF").unwrap(), vec![-1i8 as u16]);
    }

    /// MB-R-022 — a signed format accepts a `-0x` literal as the negation of the hex bit pattern.
    #[test]
    fn ut_encode_i8_neg_hex() {
        // "-0x01" → -1i8
        assert_eq!(reg(i8_be()).encode("-0x01").unwrap(), vec![-1i8 as u16]);
    }

    // --- I8 round-trip ---

    /// MB-R-007 — encoding then decoding an I8 recovers the value across its range.
    #[test]
    fn ut_roundtrip_i8() {
        let r = reg(i8_be());
        for val in [-128i8, -1, 0, 1, 127] {
            let words = r.encode(&val.to_string()).unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.to_string(), val.to_string());
        }
    }

    // --- U32 decode ---

    /// MB-R-013 — big-endian decodes the register words' byte stream in wire order.
    #[test]
    fn ut_decode_u32_big() {
        match reg(u32_be()).decode(&[0x0001, 0x0002]).unwrap() {
            Value::U32((v, _)) => assert_eq!(v, 0x00010002),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-013 — little-endian decodes the register words' byte stream fully reversed.
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

    /// MB-R-013 — big-endian encodes the value's byte stream in wire order.
    #[test]
    fn ut_encode_u32_big() {
        // 65538 = 0x00010002
        assert_eq!(
            reg(u32_be()).encode("65538").unwrap(),
            vec![0x0001u16, 0x0002u16]
        );
    }

    /// MB-R-022 — a `0x`-prefixed hex literal is accepted for numeric input.
    #[test]
    fn ut_encode_u32_big_hex() {
        assert_eq!(
            reg(u32_be()).encode("0x00010002").unwrap(),
            vec![0x0001u16, 0x0002u16]
        );
    }

    /// MB-R-013 — little-endian encodes the value's byte stream fully reversed.
    #[test]
    fn ut_encode_u32_little() {
        // 0x00010002 in LE bytes: [0x02, 0x00, 0x01, 0x00] → registers [0x0200, 0x0100]
        assert_eq!(
            reg(u32_le()).encode("65538").unwrap(),
            vec![0x0200u16, 0x0100u16]
        );
    }

    // --- U32 round-trip ---

    /// MB-R-007 — encoding then decoding a big-endian U32 recovers the value.
    #[test]
    fn ut_roundtrip_u32_big() {
        let r = reg(u32_be());
        let words = r.encode("123456789").unwrap();
        let decoded = r.decode(&words).unwrap();
        assert_eq!(decoded.to_string(), "123456789");
    }

    /// MB-R-007 — encoding then decoding a little-endian U32 recovers the value.
    #[test]
    fn ut_roundtrip_u32_little() {
        let r = reg(u32_le());
        let words = r.encode("987654321").unwrap();
        let decoded = r.decode(&words).unwrap();
        assert_eq!(decoded.to_string(), "987654321");
    }

    // --- I32 round-trip ---

    /// MB-R-007 — encoding then decoding a big-endian I32 recovers the value across its range.
    #[test]
    fn ut_roundtrip_i32_big() {
        let r = reg(i32_be());
        for val in [-2147483648i32, -1, 0, 1, 2147483647] {
            let words = r.encode(&val.to_string()).unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.to_string(), val.to_string(), "val={}", val);
        }
    }

    /// MB-R-007 — encoding then decoding a little-endian I32 recovers the value across its range.
    #[test]
    fn ut_roundtrip_i32_little() {
        let r = reg(i32_le());
        for val in [-2147483648i32, -1, 0, 1, 2147483647] {
            let words = r.encode(&val.to_string()).unwrap();
            let decoded = r.decode(&words).unwrap();
            assert_eq!(decoded.to_string(), val.to_string(), "val={}", val);
        }
    }

    /// MB-R-022 — a signed format accepts a `-0x` literal as the negation of the hex bit pattern.
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

    /// MB-R-022 — a plain `0x` literal on a signed format is taken as the two's-complement bit pattern.
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

    /// MB-R-018 — a float decodes from its raw IEEE 754 bit pattern.
    #[test]
    fn ut_decode_f32_big() {
        let bits = 1.5f32.to_bits();
        let words = vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16];
        match reg(f32_be()).decode(&words).unwrap() {
            Value::F32((f, _)) => assert!((f - 1.5f32).abs() < 1e-6),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-018 — a float encodes as its raw IEEE 754 bit pattern.
    #[test]
    fn ut_encode_f32_decimal() {
        let bits = 1.5f32.to_bits();
        let expected = vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16];
        assert_eq!(reg(f32_be()).encode("1.5").unwrap(), expected);
    }

    /// MB-R-018 — encoding then decoding an F32 recovers the value via its bit pattern.
    #[test]
    fn ut_roundtrip_f32_big() {
        let r = reg(f32_be());
        let words = r.encode("1.5").unwrap();
        match r.decode(&words).unwrap() {
            Value::F32((f, _)) => assert!((f - 1.5f32).abs() < 1e-6),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-022 — a `0x` literal on a float format is taken as its IEEE 754 bit pattern.
    #[test]
    fn ut_encode_f32_hex() {
        let bits = 1.5f32.to_bits();
        let hex_str = format!("0x{:08X}", bits);
        let expected = vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16];
        assert_eq!(reg(f32_be()).encode(&hex_str).unwrap(), expected);
    }

    // --- F64 ---

    /// MB-R-018 — encoding then decoding an F64 recovers the value via its bit pattern.
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

    /// MB-R-019 — `Ascii` packs two characters per register.
    #[test]
    fn ut_decode_ascii_exact_fill() {
        // "ABCD" fills exactly 4 bytes (Width(2) = 2 registers = 4 bytes)
        let r = reg(Format::Ascii((Alignment::Left, Width(2))));
        match r.decode(&[0x4142, 0x4344]).unwrap() {
            Value::Ascii(s) => assert_eq!(s, "ABCD"),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-020 — `Left` alignment zero-pads on the right to `2 × width` bytes.
    #[test]
    fn ut_encode_ascii_left_aligned() {
        // "AB" left-aligned in 4 bytes: [0x41, 0x42, 0x00, 0x00]
        let r = reg(Format::Ascii((Alignment::Left, Width(2))));
        assert_eq!(r.encode("AB").unwrap(), vec![0x4142u16, 0x0000u16]);
    }

    /// MB-R-020 — `Right` alignment zero-pads on the left to `2 × width` bytes.
    #[test]
    fn ut_encode_ascii_right_aligned() {
        // "AB" right-aligned in 4 bytes: [0x00, 0x00, 0x41, 0x42]
        let r = reg(Format::Ascii((Alignment::Right, Width(2))));
        assert_eq!(r.encode("AB").unwrap(), vec![0x0000u16, 0x4142u16]);
    }

    /// MB-R-007 — encoding then decoding an exact-fill ASCII value recovers the string.
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

    /// MB-R-021 — the display resolution scales the shown value but not the words on the wire.
    #[test]
    fn ut_decode_u16_with_resolution() {
        let r = reg(Format::U16((Endian::Big, Resolution(0.5), bf())));
        let words = r.encode("2048").unwrap();
        let decoded = r.decode(&words).unwrap();
        // 2048 * 0.5 = 1024.0
        assert_eq!(decoded.to_string(), "1024");
    }

    // --- Bit-field mask + derived shift ---

    fn u16_be_mask(mask: u128) -> Format {
        Format::U16((Endian::Big, res(), BitField { mask }))
    }

    /// MB-R-015 — decoding an integer field yields `(raw & mask) >> shift`.
    #[test]
    fn ut_decode_u16_high_byte_field() {
        // mask 0xFF00 → shift 8: raw 0xAB12 reads as 0xAB.
        match reg(u16_be_mask(0xFF00)).decode(&[0xAB12]).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0xAB),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-015 — a low-byte mask (shift 0) decodes as `raw & mask`.
    #[test]
    fn ut_decode_u16_low_byte_field() {
        // mask 0x00FF → shift 0: raw 0xAB12 reads as 0x12.
        match reg(u16_be_mask(0x00FF)).decode(&[0xAB12]).unwrap() {
            Value::U16((v, _)) => assert_eq!(v, 0x12),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-015 — encoding a field places the value as `(value << shift) & mask`, other bits zero.
    #[test]
    fn ut_encode_u16_high_byte_field() {
        // value 0x12 placed into mask 0xFF00 → word 0x1200, other bits zero.
        assert_eq!(
            reg(u16_be_mask(0xFF00)).encode("0x12").unwrap(),
            vec![0x1200u16]
        );
    }

    /// MB-R-015 — encoding then decoding a bit-field value recovers the field.
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

    /// MB-R-014 — the full-width default mask is a no-op on encode and decode.
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

    /// MB-R-009 — a register exposes a per-word write mask selecting exactly the bits it owns.
    #[test]
    fn ut_mask_words_layout() {
        // U16 mask laid out as a single word.
        assert_eq!(reg(u16_be_mask(0xFF00)).write_mask(), vec![0xFF00u16]);
        // Full U16 mask narrows to 0xFFFF.
        assert_eq!(reg(u16_be_mask(u128::MAX)).write_mask(), vec![0xFFFFu16]);
        // U32 big-endian mask spans two words.
        let r = reg(Format::U32((
            Endian::Big,
            res(),
            BitField { mask: 0xFFFF_0000 },
        )));
        assert_eq!(r.write_mask(), vec![0xFFFFu16, 0x0000u16]);
        // U8 full mask only owns the low byte of its word.
        assert_eq!(reg(u8_be()).write_mask(), vec![0x00FFu16]);
    }

    // --- Wider integer variants, both endians (decode/encode/mask) ---

    /// MB-R-007 — encoding then decoding the wide integer formats recovers the value in both byte orders.
    #[test]
    fn ut_roundtrip_wide_ints() {
        let e = [Endian::Big, Endian::Little];
        for endian in e {
            let u64f = Format::U64((endian.clone(), res(), bf()));
            let u128f = Format::U128((endian.clone(), res(), bf()));
            let i16f = Format::I16((endian.clone(), res(), bf()));
            let i64f = Format::I64((endian.clone(), res(), bf()));
            let i128f = Format::I128((endian.clone(), res(), bf()));

            // Display scales through f64, so values stay within f64's exact
            // integer range (< 2^53) to survive the round-trip string compare.
            for (f, s) in [
                (u64f, "1234567890123"),
                (u128f, "123456789012345"),
                (i16f, "-12345"),
                (i64f, "-1234567890123"),
                (i128f, "-123456789012345"),
            ] {
                let r = reg(f);
                let words = r.encode(s).unwrap();
                assert_eq!(r.decode(&words).unwrap().to_string(), s);
            }
        }
    }

    /// MB-R-018 — floats encode/decode via their bit pattern under little-endian byte order.
    #[test]
    fn ut_roundtrip_floats_little_endian() {
        let f32le = reg(Format::F32((Endian::Little, res())));
        let words = f32le.encode("1.5").unwrap();
        match f32le.decode(&words).unwrap() {
            Value::F32((f, _)) => assert!((f - 1.5f32).abs() < 1e-6),
            _ => panic!("Wrong variant"),
        }

        let f64le = reg(Format::F64((Endian::Little, res())));
        let words = f64le.encode("2.25").unwrap();
        match f64le.decode(&words).unwrap() {
            Value::F64((f, _)) => assert!((f - 2.25f64).abs() < 1e-10),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-022 — a `0x` literal on an F64 format is taken as its IEEE 754 bit pattern.
    #[test]
    fn ut_encode_f64_hex() {
        let bits = 2.5f64.to_bits();
        let hex_str = format!("0x{:016X}", bits);
        let r = reg(f64_be());
        let words = r.encode(&hex_str).unwrap();
        match r.decode(&words).unwrap() {
            Value::F64((f, _)) => assert!((f - 2.5f64).abs() < 1e-10),
            _ => panic!("Wrong variant"),
        }
    }

    /// MB-R-012 — a little-endian I8 places the byte in the register's high byte.
    #[test]
    fn ut_encode_i8_little_endian() {
        // I8 little-endian places the byte in the high byte of the word.
        let r = reg(Format::I8((Endian::Little, res(), bf())));
        assert_eq!(r.encode("-1").unwrap(), vec![(-1i8 as u16) << 8]);
    }

    /// MB-R-009 — the per-word write mask is laid out correctly across every format width.
    #[test]
    fn ut_mask_words_all_widths() {
        // U8 little-endian mask sits in the high byte.
        assert_eq!(reg(u8_le()).write_mask(), vec![0xFF00u16]);
        // U64 / U128 masks span multiple words.
        assert_eq!(
            reg(Format::U64((
                Endian::Big,
                res(),
                BitField {
                    mask: u64::MAX as u128
                }
            )))
            .write_mask(),
            vec![0xFFFFu16; 4]
        );
        assert_eq!(
            reg(Format::U128((
                Endian::Big,
                res(),
                BitField { mask: u128::MAX }
            )))
            .write_mask(),
            vec![0xFFFFu16; 8]
        );
        // Float / ASCII masks are all-ones across their width.
        assert_eq!(reg(f32_be()).write_mask(), vec![0xFFFFu16; 2]);
        assert_eq!(
            reg(Format::Ascii((Alignment::Left, Width(3)))).write_mask(),
            vec![0xFFFFu16; 3]
        );
    }

    /// MB-R-009 — the merge `(old & !mask) | (new & mask)` preserves bits owned by sibling registers.
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

    // --- Typed encode_value: equivalence with the string path ---

    /// MB-R-007 — encoding a typed value produces the same words as encoding the equivalent string.
    #[test]
    fn ut_encode_value_matches_string_path_all_variants() {
        let e = [Endian::Big, Endian::Little];
        for endian in e {
            let cases: Vec<(Format, Value, &str)> = vec![
                (
                    Format::U8((endian.clone(), res(), bf())),
                    Value::U8((200, res())),
                    "200",
                ),
                (
                    Format::I8((endian.clone(), res(), bf())),
                    Value::I8((-1, res())),
                    "-1",
                ),
                (
                    Format::U16((endian.clone(), res(), bf())),
                    Value::U16((1234, res())),
                    "1234",
                ),
                (
                    Format::I16((endian.clone(), res(), bf())),
                    Value::I16((-1234, res())),
                    "-1234",
                ),
                (
                    Format::U32((endian.clone(), res(), bf())),
                    Value::U32((123456789, res())),
                    "123456789",
                ),
                (
                    Format::I32((endian.clone(), res(), bf())),
                    Value::I32((-123456789, res())),
                    "-123456789",
                ),
                (
                    Format::U64((endian.clone(), res(), bf())),
                    Value::U64((1234567890123, res())),
                    "1234567890123",
                ),
                (
                    Format::I64((endian.clone(), res(), bf())),
                    Value::I64((-1234567890123, res())),
                    "-1234567890123",
                ),
                (
                    Format::U128((endian.clone(), res(), bf())),
                    Value::U128((123456789012345, res())),
                    "123456789012345",
                ),
                (
                    Format::I128((endian.clone(), res(), bf())),
                    Value::I128((-123456789012345, res())),
                    "-123456789012345",
                ),
                (
                    Format::F32((endian.clone(), res())),
                    Value::F32((1.5, res())),
                    "1.5",
                ),
                (
                    Format::F64((endian.clone(), res())),
                    Value::F64((2.25, res())),
                    "2.25",
                ),
            ];
            for (format, value, s) in cases {
                let via_string = encode(&format, s).unwrap();
                let via_typed = encode_value(&format, &value).unwrap();
                assert_eq!(via_string, via_typed, "format={:?}", format);
            }
        }
        // ASCII
        let ascii = Format::Ascii((Alignment::Left, Width(2)));
        assert_eq!(
            encode(&ascii, "AB").unwrap(),
            encode_value(&ascii, &Value::Ascii("AB".to_string())).unwrap()
        );
    }

    /// MB-R-021 — the typed encode path applies the bit-field but not the resolution, matching the wire words.
    #[test]
    fn ut_encode_value_bitfield_and_resolution() {
        // Bit-field placement applies identically via the typed path.
        let format = u16_be_mask(0x0FF0);
        let words = encode_value(&format, &Value::U16((0xAB, res()))).unwrap();
        assert_eq!(words, vec![0x0AB0u16]);

        // Resolution is not applied by encode_value, same as the string path.
        let format = Format::U16((Endian::Big, Resolution(0.5), bf()));
        let words = encode_value(&format, &Value::U16((2048, Resolution(0.5)))).unwrap();
        assert_eq!(words, encode(&format, "2048").unwrap());
    }

    /// MB-R-007 — encoding a typed value then decoding recovers it.
    #[test]
    fn ut_encode_value_roundtrip_via_decode() {
        let r = reg(i32_le());
        for val in [-2147483648i32, -1, 0, 1, 2147483647] {
            let words = r.encode_value(&Value::I32((val, res()))).unwrap();
            match r.decode(&words).unwrap() {
                Value::I32((v, _)) => assert_eq!(v, val),
                _ => panic!("Wrong variant"),
            }
        }
    }

    // --- Bit-field mask width validation ---

    /// MB-R-016 — a mask setting a bit at or above the format width is rejected on decode.
    #[test]
    fn ut_decode_bitfield_mask_out_of_width_errors() {
        // 0x1FF on a U8 sets bit 8, outside the 8-bit width.
        let format = Format::U8((Endian::Big, res(), BitField { mask: 0x1FF }));
        let err = decode(&format, &[0x0001]).unwrap_err();
        assert!(matches!(err, CodecError::BitFieldWidth(_)));

        // 0xFF00 on a U8 masks out every bit of the 8-bit value entirely.
        let format = Format::U8((Endian::Big, res(), BitField { mask: 0xFF00 }));
        let err = decode(&format, &[0x0001]).unwrap_err();
        assert!(matches!(err, CodecError::BitFieldWidth(_)));
    }

    /// MB-R-016 — a mask setting a bit at or above the format width is rejected on encode.
    #[test]
    fn ut_encode_bitfield_mask_out_of_width_errors() {
        let format = Format::U8((Endian::Big, res(), BitField { mask: 0x1FF }));
        let err = encode(&format, "1").unwrap_err();
        assert!(matches!(err, CodecError::BitFieldWidth(_)));

        let err = encode_value(&format, &Value::U8((1, res()))).unwrap_err();
        assert!(matches!(err, CodecError::BitFieldWidth(_)));
    }

    /// MB-R-016 — masks within the format width are accepted on decode and encode.
    #[test]
    fn ut_bitfield_mask_in_width_still_works() {
        // Existing in-width masks keep decoding/encoding as before.
        assert!(reg(u16_be_mask(0xFF00)).decode(&[0xAB12]).is_ok());
        assert!(reg(u16_be_mask(0x0FF0)).encode("0xAB").is_ok());
        assert!(reg(u8_be()).decode(&[0x00FF]).is_ok());
    }

    /// MB-R-008 — encoding a typed value whose type does not match the format fails with a mismatch error.
    #[test]
    fn ut_encode_value_mismatch_errors() {
        let format = u32_be();
        let err = encode_value(&format, &Value::U16((1, res()))).unwrap_err();
        assert!(matches!(err, CodecError::ValueFormatMismatch(_)));

        let ascii = Format::Ascii((Alignment::Left, Width(2)));
        let err = encode_value(&ascii, &Value::U8((1, res()))).unwrap_err();
        assert!(matches!(err, CodecError::ValueFormatMismatch(_)));
    }
}
