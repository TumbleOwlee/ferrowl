//! Register edit dialogs and their shared parsing/formatting helpers.

mod add_value;
mod confirm;
mod input;
mod selection;
mod subdialog;

pub use add_value::*;
pub use confirm::*;
pub use input::*;
pub use selection::*;
pub use subdialog::*;

use ferrowl_codec::format::{
    Alignment as TextAlignment, BitField, Endian as RegisterEndian, Format as RegisterFormat,
    Resolution,
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

/// Index into the `number_format` selection (order matches dialog `new()`).
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

pub(super) fn numeric_parts(format: &RegisterFormat) -> (RegisterEndian, Resolution, BitField) {
    match format {
        RegisterFormat::U8((e, r, bf))
        | RegisterFormat::U16((e, r, bf))
        | RegisterFormat::U32((e, r, bf))
        | RegisterFormat::U64((e, r, bf))
        | RegisterFormat::U128((e, r, bf))
        | RegisterFormat::I8((e, r, bf))
        | RegisterFormat::I16((e, r, bf))
        | RegisterFormat::I32((e, r, bf))
        | RegisterFormat::I64((e, r, bf))
        | RegisterFormat::I128((e, r, bf)) => (e.clone(), r.clone(), bf.clone()),
        RegisterFormat::F32((e, r)) | RegisterFormat::F64((e, r)) => {
            (e.clone(), r.clone(), BitField::default())
        }
        RegisterFormat::Ascii(_) => (RegisterEndian::Big, Resolution(1.0), BitField::default()),
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
/// endian/resolution. Integer variants also carry `bitfield`; floats ignore it.
pub(super) fn with_numeric_parts(
    format: &RegisterFormat,
    endian: RegisterEndian,
    resolution: Resolution,
    bitfield: BitField,
) -> RegisterFormat {
    let int = (endian.clone(), resolution.clone(), bitfield);
    let float = (endian, resolution);
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
