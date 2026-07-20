//! Register edit dialogs and their shared parsing/formatting helpers.

mod add_value;
mod confirm;
mod input;
mod register_dialog;
mod selection;
mod subdialog;
mod widgets;

pub use add_value::*;
pub use confirm::*;
pub use input::*;
pub use register_dialog::*;
pub use selection::*;
pub use subdialog::*;

use ferrowl_codec::format::{
    Alignment as TextAlignment, BitField, Endian as RegisterEndian, Format as RegisterFormat,
    Resolution, WordOrder as RegisterWordOrder,
};
use ferrowl_codec::{Access, Address, Kind};

/// Parse an address-field value: the literal `virtual` (case-insensitive) selects a virtual
/// register (no Modbus address), otherwise a `u16` address.
pub(super) fn parse_address(input: &str) -> Result<Address, String> {
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("virtual") {
        Ok(Address::Virtual)
    } else {
        trimmed
            .parse::<u16>()
            .map(Address::Fixed)
            .map_err(|_| "Address must be a number or 'virtual'.".to_string())
    }
}
use ferrowl_ui::{
    state::InputFieldState,
    traits::ToLabel,
    widgets::{InputField, Validate, Widget},
};
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct Format(RegisterFormat);

impl ToLabel for Format {
    fn to_label(&self) -> String {
        match self.0 {
            RegisterFormat::U8(_) => "U8",
            RegisterFormat::U16(_) => "U16",
            RegisterFormat::U32(_) => "U32",
            RegisterFormat::U64(_) => "U64",
            RegisterFormat::U128(_) => "U128",
            RegisterFormat::I8(_) => "I8",
            RegisterFormat::I16(_) => "I16",
            RegisterFormat::I32(_) => "I32",
            RegisterFormat::I64(_) => "I64",
            RegisterFormat::I128(_) => "I128",
            RegisterFormat::F32(_) => "F32",
            RegisterFormat::F64(_) => "F64",
            RegisterFormat::Ascii(_) => "ASCII",
        }
        .to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Endian(RegisterEndian);

impl ToLabel for Endian {
    fn to_label(&self) -> String {
        match self.0 {
            RegisterEndian::Big => "Big",
            RegisterEndian::Little => "Little",
        }
        .to_string()
    }
}

#[derive(Debug, Clone)]
pub struct WordOrder(RegisterWordOrder);

impl ToLabel for WordOrder {
    fn to_label(&self) -> String {
        match self.0 {
            RegisterWordOrder::Normal => "Normal",
            RegisterWordOrder::Reversed => "Reversed",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    Number,
    Text,
}

impl ToLabel for ValueType {
    fn to_label(&self) -> String {
        match self {
            ValueType::Number => "Number",
            ValueType::Text => "Text",
        }
        .to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Alignment(TextAlignment);

impl ToLabel for Alignment {
    fn to_label(&self) -> String {
        match self.0 {
            TextAlignment::Right => "Right",
            TextAlignment::Left => "Left",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindOption(pub Kind);

impl ToLabel for KindOption {
    fn to_label(&self) -> String {
        match self.0 {
            Kind::Coil => "Coil",
            Kind::DiscreteInput => "Discrete Input",
            Kind::HoldingRegister => "Holding Register",
            Kind::InputRegister => "Input Register",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessOption(pub Access);

impl ToLabel for AccessOption {
    fn to_label(&self) -> String {
        match self.0 {
            Access::ReadOnly => "Read Only",
            Access::WriteOnly => "Write Only",
            Access::ReadWrite => "Read Write",
        }
        .to_string()
    }
}

/// Index into the `access` selection (order matches dialog `new()`).
pub(super) fn access_index(access: &Access) -> usize {
    match access {
        Access::ReadOnly => 0,
        Access::WriteOnly => 1,
        Access::ReadWrite => 2,
    }
}

/// Index into the `kind` selection (order matches dialog `new()`).
pub(super) fn kind_index(kind: &Kind) -> usize {
    match kind {
        Kind::Coil => 0,
        Kind::DiscreteInput => 1,
        Kind::HoldingRegister => 2,
        Kind::InputRegister => 3,
    }
}

pub(super) fn set_input<T: Validate>(
    widget: &mut Widget<InputFieldState, InputField<T>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_autofill(Some(value.to_string()));
    widget.state.set_cursor(value.chars().count());
}

pub(super) fn alignment_index(alignment: &TextAlignment) -> usize {
    match alignment {
        TextAlignment::Left => 0,
        TextAlignment::Right => 1,
    }
}

pub(super) fn endian_index(endian: &RegisterEndian) -> usize {
    match endian {
        RegisterEndian::Big => 0,
        RegisterEndian::Little => 1,
    }
}

pub(super) fn word_order_index(word_order: &RegisterWordOrder) -> usize {
    match word_order {
        RegisterWordOrder::Normal => 0,
        RegisterWordOrder::Reversed => 1,
    }
}

/// Index into the `number_format` selection (order matches dialog `new()`).
///
/// `RegisterFormat::Ascii` has no slot in `number_format` (ASCII is a distinct `value_type`, not a
/// number format). In this codebase both callers already match `Ascii` out separately before
/// falling through to the numeric arm that calls `format_index`, so this function is never
/// actually invoked with `Ascii` in practice. It still maps `Ascii` to `0` (same as `U8`) as a
/// well-defined, documented fallback rather than an unreachable/panic path, so a future caller
/// that forgets to gate on value type first degrades gracefully instead of panicking.
pub(super) fn format_index(format: &RegisterFormat) -> usize {
    match format {
        RegisterFormat::U8(_) => 0,
        RegisterFormat::U16(_) => 1,
        RegisterFormat::U32(_) => 2,
        RegisterFormat::U64(_) => 3,
        RegisterFormat::U128(_) => 4,
        RegisterFormat::I8(_) => 5,
        RegisterFormat::I16(_) => 6,
        RegisterFormat::I32(_) => 7,
        RegisterFormat::I64(_) => 8,
        RegisterFormat::I128(_) => 9,
        RegisterFormat::F32(_) => 10,
        RegisterFormat::F64(_) => 11,
        RegisterFormat::Ascii(_) => 0,
    }
}

pub(super) fn numeric_parts(
    format: &RegisterFormat,
) -> (RegisterEndian, RegisterWordOrder, Resolution, BitField) {
    match format {
        RegisterFormat::U8((e, w, r, bf))
        | RegisterFormat::U16((e, w, r, bf))
        | RegisterFormat::U32((e, w, r, bf))
        | RegisterFormat::U64((e, w, r, bf))
        | RegisterFormat::U128((e, w, r, bf))
        | RegisterFormat::I8((e, w, r, bf))
        | RegisterFormat::I16((e, w, r, bf))
        | RegisterFormat::I32((e, w, r, bf))
        | RegisterFormat::I64((e, w, r, bf))
        | RegisterFormat::I128((e, w, r, bf)) => (e.clone(), *w, r.clone(), bf.clone()),
        RegisterFormat::F32((e, w, r)) | RegisterFormat::F64((e, w, r)) => {
            (e.clone(), *w, r.clone(), BitField::default())
        }
        RegisterFormat::Ascii(_) => (
            RegisterEndian::Big,
            RegisterWordOrder::Normal,
            Resolution(1.0),
            BitField::default(),
        ),
    }
}

/// Whether `format` is an integer type (carries a [`BitField`]); false for
/// floats and ASCII. Used to gate the bitmask input field in the edit dialogs.
pub(super) fn is_integer_format(format: &RegisterFormat) -> bool {
    matches!(
        format,
        RegisterFormat::U8(_)
            | RegisterFormat::U16(_)
            | RegisterFormat::U32(_)
            | RegisterFormat::U64(_)
            | RegisterFormat::U128(_)
            | RegisterFormat::I8(_)
            | RegisterFormat::I16(_)
            | RegisterFormat::I32(_)
            | RegisterFormat::I64(_)
            | RegisterFormat::I128(_)
    )
}

/// Whether `format` occupies more than one register (`width > 1`). Used to gate
/// the register-order selector, which is inert for single-register formats.
pub(super) fn is_multi_register_format(format: &RegisterFormat) -> bool {
    format.width() > 1
}

/// Parse a bitmask input field (`0x`-prefixed hex or decimal). Empty ⇒ the full
/// no-op mask. Returns a user-facing error string on a malformed value.
pub(super) fn parse_bitmask(s: &str) -> Result<BitField, String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(BitField::default());
    }
    let mask = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u128::from_str_radix(hex, 16).map_err(|_| "must be a hex (0x…) or decimal number")?
    } else {
        t.parse::<u128>()
            .map_err(|_| "must be a hex (0x…) or decimal number")?
    };
    Ok(BitField { mask })
}

/// Rebuild a numeric format of the same variant as `format` with the given
/// endian/register-order/resolution. Integer variants also carry `bitfield`;
/// floats ignore it. Register order is inert for single-register variants.
pub(super) fn with_numeric_parts(
    format: &RegisterFormat,
    endian: RegisterEndian,
    word_order: RegisterWordOrder,
    resolution: Resolution,
    bitfield: BitField,
) -> RegisterFormat {
    let int = (endian.clone(), word_order, resolution.clone(), bitfield);
    let float = (endian, word_order, resolution);
    match format {
        RegisterFormat::U8(_) => RegisterFormat::U8(int),
        RegisterFormat::U16(_) => RegisterFormat::U16(int),
        RegisterFormat::U32(_) => RegisterFormat::U32(int),
        RegisterFormat::U64(_) => RegisterFormat::U64(int),
        RegisterFormat::U128(_) => RegisterFormat::U128(int),
        RegisterFormat::I8(_) => RegisterFormat::I8(int),
        RegisterFormat::I16(_) => RegisterFormat::I16(int),
        RegisterFormat::I32(_) => RegisterFormat::I32(int),
        RegisterFormat::I64(_) => RegisterFormat::I64(int),
        RegisterFormat::I128(_) => RegisterFormat::I128(int),
        RegisterFormat::F32(_) => RegisterFormat::F32(float),
        RegisterFormat::F64(_) => RegisterFormat::F64(float),
        RegisterFormat::Ascii(_) => RegisterFormat::U16(int),
    }
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn ut_parse_address_virtual_case_insensitive_and_numeric() {
        assert_eq!(parse_address("virtual"), Ok(Address::Virtual));
        assert_eq!(parse_address("  VIRTUAL  "), Ok(Address::Virtual));
        assert_eq!(parse_address("42"), Ok(Address::Fixed(42)));
        assert!(parse_address("nope").is_err());
        // Out of u16 range -> error.
        assert!(parse_address("70000").is_err());
    }

    #[test]
    fn ut_selection_indices_match_dialog_order() {
        assert_eq!(access_index(&Access::ReadOnly), 0);
        assert_eq!(access_index(&Access::WriteOnly), 1);
        assert_eq!(access_index(&Access::ReadWrite), 2);

        assert_eq!(kind_index(&Kind::Coil), 0);
        assert_eq!(kind_index(&Kind::DiscreteInput), 1);
        assert_eq!(kind_index(&Kind::HoldingRegister), 2);
        assert_eq!(kind_index(&Kind::InputRegister), 3);

        assert_eq!(endian_index(&RegisterEndian::Big), 0);
        assert_eq!(endian_index(&RegisterEndian::Little), 1);

        assert_eq!(alignment_index(&TextAlignment::Left), 0);
        assert_eq!(alignment_index(&TextAlignment::Right), 1);
    }

    #[test]
    fn ut_format_index_covers_all_variants() {
        let bf = BitField::default();
        let r = Resolution(1.0);
        let e = RegisterEndian::Big;
        assert_eq!(
            format_index(&RegisterFormat::U8((
                e.clone(),
                RegisterWordOrder::Normal,
                r.clone(),
                bf.clone()
            ))),
            0
        );
        assert_eq!(
            format_index(&RegisterFormat::I128((
                e.clone(),
                RegisterWordOrder::Normal,
                r.clone(),
                bf.clone()
            ))),
            9
        );
        assert_eq!(
            format_index(&RegisterFormat::F32((
                e.clone(),
                RegisterWordOrder::Normal,
                r.clone()
            ))),
            10
        );
        assert_eq!(
            format_index(&RegisterFormat::F64((
                e.clone(),
                RegisterWordOrder::Normal,
                r.clone()
            ))),
            11
        );
        // ASCII has no number-format slot and maps to index 0.
        assert_eq!(
            format_index(&RegisterFormat::Ascii((
                TextAlignment::Left,
                ferrowl_codec::format::Width(2)
            ))),
            0
        );
    }

    #[test]
    fn ut_is_integer_format_excludes_float_and_ascii() {
        let bf = BitField::default();
        let r = Resolution(1.0);
        let e = RegisterEndian::Big;
        assert!(is_integer_format(&RegisterFormat::U16((
            e.clone(),
            RegisterWordOrder::Normal,
            r.clone(),
            bf.clone()
        ))));
        assert!(is_integer_format(&RegisterFormat::I64((
            e.clone(),
            RegisterWordOrder::Normal,
            r.clone(),
            bf.clone()
        ))));
        assert!(!is_integer_format(&RegisterFormat::F32((
            e.clone(),
            RegisterWordOrder::Normal,
            r.clone()
        ))));
        assert!(!is_integer_format(&RegisterFormat::Ascii((
            TextAlignment::Left,
            ferrowl_codec::format::Width(2)
        ))));
    }

    #[test]
    fn ut_parse_bitmask_hex_decimal_empty_and_invalid() {
        assert_eq!(parse_bitmask("").unwrap().mask, BitField::default().mask);
        assert_eq!(parse_bitmask("0xFF00").unwrap().mask, 0xFF00);
        assert_eq!(parse_bitmask("0X00ff").unwrap().mask, 0x00ff);
        assert_eq!(parse_bitmask("255").unwrap().mask, 255);
        assert_eq!(parse_bitmask("  16  ").unwrap().mask, 16);
        assert!(parse_bitmask("zz").is_err());
        assert!(parse_bitmask("0xZZ").is_err());
    }

    #[test]
    fn ut_with_numeric_parts_preserves_variant_and_applies_fields() {
        let src = RegisterFormat::U32((
            RegisterEndian::Big,
            RegisterWordOrder::Normal,
            Resolution(1.0),
            BitField::default(),
        ));
        let rebuilt = with_numeric_parts(
            &src,
            RegisterEndian::Little,
            RegisterWordOrder::Reversed,
            Resolution(0.25),
            BitField { mask: 0x0F0F },
        );
        // Same variant (U32), new endian/word order/resolution/bitfield.
        assert_eq!(
            rebuilt,
            RegisterFormat::U32((
                RegisterEndian::Little,
                RegisterWordOrder::Reversed,
                Resolution(0.25),
                BitField { mask: 0x0F0F }
            ))
        );
        // Floats ignore the supplied bitfield.
        let float = with_numeric_parts(
            &RegisterFormat::F32((
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
            )),
            RegisterEndian::Little,
            RegisterWordOrder::Reversed,
            Resolution(2.0),
            BitField { mask: 0x1234 },
        );
        assert_eq!(
            float,
            RegisterFormat::F32((
                RegisterEndian::Little,
                RegisterWordOrder::Reversed,
                Resolution(2.0)
            ))
        );
    }
}
