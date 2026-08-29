//! Conversion between the Lua `ValueType` surfaced by `C_Register` and the `ferrowl-codec`
//! [`Value`]/[`Format`] the register store speaks: `virtual_value_from_type` and
//! `typed_value_from_type` for the Set path, `value_to_type` for the Get path.

use ferrowl_codec::{FloatKind, Format, IntKind, NumericPrimitive, Register, Value};
use ferrowl_lua::module::ValueType;
use ferrowl_lua::{Error, Result};

/// Converts a Lua `ValueType` into a codec [`Value`] for a virtual register, mirroring
/// `str_to_value`'s `Scalar`-based semantics (an `Int`/`Float`/`Bool` is stored as `I64`/`F64`
/// regardless of the register's declared format — virtual registers ignore it) without the
/// string round-trip `str_to_value` requires for genuinely string input.
pub(super) fn virtual_value_from_type(value: ValueType, register: &Register) -> Result<Value> {
    let res = register.format().resolution().unwrap_or_default();
    match value {
        ValueType::Nil => Err(Error::RuntimeError("cannot Set nil value".to_string())),
        ValueType::String(s) => Ok(crate::module::modbus::str_to_value(&s, register)),
        ValueType::Bool(b) => Ok(Value::i64(b as i64, res)),
        ValueType::Int(v) => Ok(match i64::try_from(v) {
            Ok(v) => Value::i64(v, res),
            // Mirrors `Scalar::from_input`'s fallback for an out-of-i64-range literal: it fails
            // to parse as `i64` and is retried as `f64`.
            Err(_) => Value::f64(v as f64, res),
        }),
        ValueType::Float(v) => Ok(Value::f64(v, res)),
    }
}

/// Converts a Lua `ValueType` into the codec [`Value`] variant `format` expects, for the
/// fixed-address `encode_value` path. `Nil` errors cleanly instead of round-tripping through the
/// literal string `"nil"`; `Int` is range-checked against the target integer width instead of
/// silently truncating.
pub(super) fn typed_value_from_type(value: ValueType, format: &Format) -> Result<Value> {
    match value {
        ValueType::Nil => Err(Error::RuntimeError("cannot Set nil value".to_string())),
        ValueType::String(_) => unreachable!("String is handled by the caller via `encode`"),
        ValueType::Bool(b) => int_value_for_format(b as i128, format),
        ValueType::Int(v) => int_value_for_format(v, format),
        ValueType::Float(v) => float_value_for_format(v, format),
    }
}

/// Builds the codec [`Value`] variant `format` expects from an integer, range-checking against
/// the target width. Mirrors the string path's rule for non-integer formats: any integer is a
/// valid float (`v as f32/f64`), and stringifies verbatim onto ASCII.
fn int_value_for_format(v: i128, format: &Format) -> Result<Value> {
    let res = format.resolution().unwrap_or_default();
    macro_rules! int_variant {
        ($variant:ident, $ty:ty) => {
            <$ty>::try_from(v)
                .map(|val| Value::$variant(val, res.clone()))
                .map_err(|_| {
                    Error::RuntimeError(format!("value {v} out of range for format {format}"))
                })
        };
    }
    match format {
        Format::Numeric(nf) => match nf.kind {
            IntKind::U8 => int_variant!(u8, u8),
            IntKind::U16 => int_variant!(u16, u16),
            IntKind::U32 => int_variant!(u32, u32),
            IntKind::U64 => int_variant!(u64, u64),
            IntKind::U128 => u128::try_from(v)
                .map(|val| Value::u128(val, res.clone()))
                .map_err(|_| {
                    Error::RuntimeError(format!("value {v} out of range for format {format}"))
                }),
            IntKind::I8 => int_variant!(i8, i8),
            IntKind::I16 => int_variant!(i16, i16),
            IntKind::I32 => int_variant!(i32, i32),
            IntKind::I64 => int_variant!(i64, i64),
            IntKind::I128 => Ok(Value::i128(v, res)),
        },
        Format::Float(ff) => match ff.kind {
            FloatKind::F32 => Ok(Value::f32(v as f32, res)),
            FloatKind::F64 => Ok(Value::f64(v as f64, res)),
        },
        Format::Ascii(_, _) => Ok(Value::Ascii(v.to_string())),
    }
}

/// Builds the codec [`Value`] variant `format` expects from a float. Mirrors the string path's
/// rule for integer formats: only a whole number in range converts; a fractional or non-finite
/// value errors cleanly instead (the string path would have failed the same conversion via a
/// confusing `ParseIntError`, since `v.to_string()` of e.g. `3.5` isn't valid integer syntax).
fn float_value_for_format(v: f64, format: &Format) -> Result<Value> {
    let res = format.resolution().unwrap_or_default();
    macro_rules! float_int_variant {
        ($variant:ident, $ty:ty) => {{
            if !v.is_finite() || v.fract() != 0.0 {
                Err(Error::RuntimeError(format!(
                    "value {v} is not a whole number for integer format {format}"
                )))
            } else if v < <$ty>::MIN as f64 || v > <$ty>::MAX as f64 {
                Err(Error::RuntimeError(format!(
                    "value {v} out of range for format {format}"
                )))
            } else {
                Ok(Value::$variant(v as $ty, res.clone()))
            }
        }};
    }
    match format {
        Format::Numeric(nf) => match nf.kind {
            IntKind::U8 => float_int_variant!(u8, u8),
            IntKind::U16 => float_int_variant!(u16, u16),
            IntKind::U32 => float_int_variant!(u32, u32),
            IntKind::U64 => float_int_variant!(u64, u64),
            IntKind::U128 => float_int_variant!(u128, u128),
            IntKind::I8 => float_int_variant!(i8, i8),
            IntKind::I16 => float_int_variant!(i16, i16),
            IntKind::I32 => float_int_variant!(i32, i32),
            IntKind::I64 => float_int_variant!(i64, i64),
            IntKind::I128 => float_int_variant!(i128, i128),
        },
        Format::Float(ff) => match ff.kind {
            FloatKind::F32 => Ok(Value::f32(v as f32, res)),
            FloatKind::F64 => Ok(Value::f64(v, res)),
        },
        Format::Ascii(_, _) => Ok(Value::Ascii(v.to_string())),
    }
}

/// Map a decoded register `Value` to the Lua `ValueType`, exposing the raw (unscaled) stored
/// number so that a subsequent `Set` round-trips through `Register::encode`.
pub(super) fn value_to_type(value: Value) -> ValueType {
    match value {
        Value::Numeric(p, _) => match p {
            NumericPrimitive::U8(v) => ValueType::Int(v as i128),
            NumericPrimitive::U16(v) => ValueType::Int(v as i128),
            NumericPrimitive::U32(v) => ValueType::Int(v as i128),
            NumericPrimitive::U64(v) => ValueType::Int(v as i128),
            NumericPrimitive::U128(v) => ValueType::Int(v as i128),
            NumericPrimitive::I8(v) => ValueType::Int(v as i128),
            NumericPrimitive::I16(v) => ValueType::Int(v as i128),
            NumericPrimitive::I32(v) => ValueType::Int(v as i128),
            NumericPrimitive::I64(v) => ValueType::Int(v as i128),
            NumericPrimitive::I128(v) => ValueType::Int(v),
            NumericPrimitive::F32(v) => ValueType::Float(v as f64),
            NumericPrimitive::F64(v) => ValueType::Float(v),
        },
        Value::Ascii(s) => ValueType::String(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_codec::format::{Alignment, BitField, Endian, Resolution, Width, WordOrder};
    use ferrowl_codec::{Access, Address, Kind, RegisterBuilder};
    use ferrowl_modbus::UnitId;

    fn int_fmt(kind: fn(Endian, WordOrder, Resolution, BitField) -> Format) -> Format {
        kind(
            Endian::Big,
            WordOrder::Normal,
            Resolution(1.0),
            BitField::default(),
        )
    }

    #[test]
    /// SC-R-027 — a read returns each stored value as its natural Lua type.
    fn ut_value_to_type_natural_types() {
        assert!(matches!(
            value_to_type(Value::u16(7, Resolution(1.0))),
            ValueType::Int(7)
        ));
        assert!(matches!(
            value_to_type(Value::i128(-9, Resolution(1.0))),
            ValueType::Int(-9)
        ));
        assert!(matches!(
            value_to_type(Value::f64(1.5, Resolution(1.0))),
            ValueType::Float(f) if f == 1.5
        ));
        assert!(matches!(
            value_to_type(Value::Ascii("hi".into())),
            ValueType::String(ref s) if s == "hi"
        ));
    }

    #[test]
    /// SC-R-027 — an integer write range-checks against the target width instead of truncating.
    fn ut_typed_value_int_range_checked() {
        let u8f = int_fmt(Format::u8);
        assert!(matches!(
            typed_value_from_type(ValueType::Int(200), &u8f).unwrap(),
            Value::Numeric(NumericPrimitive::U8(200), _)
        ));
        assert!(typed_value_from_type(ValueType::Int(300), &u8f).is_err()); // out of u8 range
        // A bool writes as 0/1 through the integer path.
        assert!(matches!(
            typed_value_from_type(ValueType::Bool(true), &u8f).unwrap(),
            Value::Numeric(NumericPrimitive::U8(1), _)
        ));
        // Nil always errors rather than coercing.
        assert!(typed_value_from_type(ValueType::Nil, &u8f).is_err());
    }

    #[test]
    /// SC-R-027 — a float write into an integer format only accepts a whole, in-range number.
    fn ut_typed_value_float_into_int_requires_whole() {
        let i16f = int_fmt(Format::i16);
        assert!(matches!(
            typed_value_from_type(ValueType::Float(42.0), &i16f).unwrap(),
            Value::Numeric(NumericPrimitive::I16(42), _)
        ));
        assert!(typed_value_from_type(ValueType::Float(3.5), &i16f).is_err()); // fractional
        assert!(typed_value_from_type(ValueType::Float(f64::NAN), &i16f).is_err()); // non-finite
        assert!(typed_value_from_type(ValueType::Float(1e9), &i16f).is_err()); // out of i16 range
    }

    #[test]
    fn ut_typed_value_float_and_ascii_formats() {
        let f32f = Format::f32(Endian::Big, WordOrder::Normal, Resolution(1.0));
        assert!(matches!(
            typed_value_from_type(ValueType::Float(1.25), &f32f).unwrap(),
            Value::Numeric(NumericPrimitive::F32(v), _) if v == 1.25
        ));
        let ascii = Format::Ascii(Alignment::Left, Width(4));
        assert!(matches!(
            typed_value_from_type(ValueType::Int(12), &ascii).unwrap(),
            Value::Ascii(ref s) if s == "12"
        ));
    }

    #[test]
    /// SC-R-027 — a virtual-register write stores the value as I64/F64 regardless of format, and
    /// nil fails.
    fn ut_virtual_value_from_type() {
        let register = RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Virtual)
            .format(int_fmt(Format::u16))
            .build()
            .unwrap();
        assert!(matches!(
            virtual_value_from_type(ValueType::Bool(true), &register).unwrap(),
            Value::Numeric(NumericPrimitive::I64(1), _)
        ));
        assert!(matches!(
            virtual_value_from_type(ValueType::Int(5), &register).unwrap(),
            Value::Numeric(NumericPrimitive::I64(5), _)
        ));
        // An out-of-i64 literal falls back to F64, mirroring the string path.
        assert!(matches!(
            virtual_value_from_type(ValueType::Int(i128::MAX), &register).unwrap(),
            Value::Numeric(NumericPrimitive::F64(_), _)
        ));
        assert!(matches!(
            virtual_value_from_type(ValueType::Float(2.5), &register).unwrap(),
            Value::Numeric(NumericPrimitive::F64(v), _) if v == 2.5
        ));
        assert!(virtual_value_from_type(ValueType::Nil, &register).is_err());
    }
}
