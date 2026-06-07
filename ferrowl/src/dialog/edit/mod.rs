mod input;
mod selection;

pub use input::*;
pub use selection::*;

use ferrowl_reg::format::{
    Alignment as TextAlignment, Endian as RegisterEndian, Format as RegisterFormat, Resolution,
};
use ferrowl_reg::Kind;
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
    widget.state.set_cursor(value.chars().count());
}

pub(super) fn alignment_index(alignment: &TextAlignment) -> usize {
    match alignment {
        TextAlignment::Right => 0,
        TextAlignment::Left => 1,
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

pub(super) fn numeric_parts(format: &RegisterFormat) -> (RegisterEndian, Resolution) {
    match format {
        RegisterFormat::U8((e, r))
        | RegisterFormat::U16((e, r))
        | RegisterFormat::U32((e, r))
        | RegisterFormat::U64((e, r))
        | RegisterFormat::U128((e, r))
        | RegisterFormat::I8((e, r))
        | RegisterFormat::I16((e, r))
        | RegisterFormat::I32((e, r))
        | RegisterFormat::I64((e, r))
        | RegisterFormat::I128((e, r))
        | RegisterFormat::F32((e, r))
        | RegisterFormat::F64((e, r)) => (e.clone(), r.clone()),
        RegisterFormat::Ascii(_) => (RegisterEndian::Big, Resolution(1.0)),
    }
}

/// Rebuild a numeric format of the same variant as `format` with the given endian/resolution.
pub(super) fn with_endian_resolution(
    format: &RegisterFormat,
    endian: RegisterEndian,
    resolution: Resolution,
) -> RegisterFormat {
    let pair = (endian, resolution);
    match format {
        RegisterFormat::U8(_) => RegisterFormat::U8(pair),
        RegisterFormat::U16(_) => RegisterFormat::U16(pair),
        RegisterFormat::U32(_) => RegisterFormat::U32(pair),
        RegisterFormat::U64(_) => RegisterFormat::U64(pair),
        RegisterFormat::U128(_) => RegisterFormat::U128(pair),
        RegisterFormat::I8(_) => RegisterFormat::I8(pair),
        RegisterFormat::I16(_) => RegisterFormat::I16(pair),
        RegisterFormat::I32(_) => RegisterFormat::I32(pair),
        RegisterFormat::I64(_) => RegisterFormat::I64(pair),
        RegisterFormat::I128(_) => RegisterFormat::I128(pair),
        RegisterFormat::F32(_) => RegisterFormat::F32(pair),
        RegisterFormat::F64(_) => RegisterFormat::F64(pair),
        RegisterFormat::Ascii(_) => RegisterFormat::U16(pair),
    }
}
