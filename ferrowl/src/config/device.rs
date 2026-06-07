//! Device-type config: the register definitions for one kind of device. One file = one
//! device type (no ip/port/role/name — those are per-instance, set via setup dialog / CLI).

use std::collections::BTreeMap;

use ferrowl_mem::{Range, Type};
use ferrowl_reg::{
    Access, Address, Format, Kind, Register, RegisterBuilder,
    format::{Alignment, Endian, Resolution, Width},
};
use ferrowl_ui::traits::ToLabel;
use serde::{Deserialize, Serialize};

/// A device-type configuration file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeviceConfig {
    /// Ferrowl version that wrote this file, stamped on save. Enables future compatibility
    /// shims when loading configs produced by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub comment: String,
    /// Device-level timing defaults (ms). Used when a `ModuleSpec` does not override them; the
    /// global app config is the final fallback. See `Module::resolve_timing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<usize>,
    /// Explicit address ranges a client reads in one Modbus request per function code (gaps
    /// included). When empty for a code, contiguous registers are auto-merged instead.
    #[serde(default, skip_serializing_if = "ReadRanges::is_empty")]
    pub read_ranges: ReadRanges,
    pub definitions: BTreeMap<String, RegisterDef>,
}

/// Per-function-code explicit read ranges. Each string is a comma-separated list of inclusive
/// address ranges, e.g. `"0-100,140-160"` (a bare `"5"` is the single address 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReadRanges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coils: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discrete: Option<String>,
}

impl ReadRanges {
    pub fn is_empty(&self) -> bool {
        self.holding.is_none()
            && self.input.is_none()
            && self.coils.is_none()
            && self.discrete.is_none()
    }

    /// Parsed ranges configured for `kind` (empty when none configured or unparsable).
    pub fn ranges_for(&self, kind: Kind) -> Vec<Range> {
        let spec = match kind {
            Kind::HoldingRegister => &self.holding,
            Kind::InputRegister => &self.input,
            Kind::Coil => &self.coils,
            Kind::DiscreteInput => &self.discrete,
        };
        spec.as_deref().map(parse_ranges).unwrap_or_default()
    }
}

/// Parse `"0-100,140-160"` (inclusive bounds; bare `"5"` = single address) into memory ranges.
/// Malformed or reversed entries are skipped.
fn parse_ranges(spec: &str) -> Vec<Range> {
    spec.split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            match part.split_once('-') {
                Some((a, b)) => {
                    let start = a.trim().parse::<usize>().ok()?;
                    let end = b.trim().parse::<usize>().ok()?;
                    (end >= start).then(|| Range::new(start, end - start + 1))
                }
                None => {
                    let addr = part.parse::<usize>().ok()?;
                    Some(Range::new(addr, 1))
                }
            }
        })
        .collect()
}

/// A single register definition within a device type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterDef {
    #[serde(default)]
    pub slave_id: u8,
    /// Modbus read function code: 1=Coil, 2=DiscreteInput, 3=InputRegister, 4=HoldingRegister.
    #[serde(default = "default_read_code")]
    pub read_code: u8,
    /// Start address; omitted (or `virtual = true`) marks a virtual register.
    #[serde(default)]
    pub address: Option<u16>,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    #[serde(default)]
    pub access: AccessCfg,
    #[serde(rename = "type")]
    pub value_type: ValueType,
    #[serde(default)]
    pub endian: EndianCfg,
    #[serde(default = "default_resolution")]
    pub resolution: f64,
    /// ASCII width in registers (ignored for numeric types).
    #[serde(default = "default_length")]
    pub length: usize,
    #[serde(default)]
    pub alignment: AlignmentCfg,
    /// Named values for selection-style registers (e.g. enum states).
    #[serde(default)]
    pub values: Vec<NamedValue>,
    /// Optional Lua snippet run every simulation cycle (see Lua phase).
    #[serde(default)]
    pub update: Option<String>,
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedValue {
    pub name: String,
    pub value: Scalar,
}

impl ToLabel for NamedValue {
    fn to_label(&self) -> String {
        self.name.clone()
    }
}

/// A named-value payload. Untagged so the config file can write `value = 10`, `value = 1.5`
/// or `value = "text"` without quoting numbers. Written to a register via its `Display` string,
/// which `Register::encode` then interprets per the register's own format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Int(i64),
    Float(f64),
    Text(String),
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scalar::Int(v) => write!(f, "{v}"),
            Scalar::Float(v) => write!(f, "{v}"),
            Scalar::Text(v) => write!(f, "{v}"),
        }
    }
}

impl Scalar {
    /// Infer a scalar from user input: integer first, then float, else verbatim text.
    pub fn from_input(s: &str) -> Self {
        let t = s.trim();
        if let Ok(i) = t.parse::<i64>() {
            Scalar::Int(i)
        } else if let Ok(f) = t.parse::<f64>() {
            Scalar::Float(f)
        } else {
            Scalar::Text(s.to_string())
        }
    }
}

/// The value encoding of a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    Ascii,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AccessCfg {
    ReadOnly,
    WriteOnly,
    #[default]
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum EndianCfg {
    #[default]
    Big,
    Little,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AlignmentCfg {
    #[default]
    Left,
    Right,
}

fn default_read_code() -> u8 {
    3
}
fn default_resolution() -> f64 {
    1.0
}
fn default_length() -> usize {
    1
}

impl From<AccessCfg> for Access {
    fn from(a: AccessCfg) -> Self {
        match a {
            AccessCfg::ReadOnly => Access::ReadOnly,
            AccessCfg::WriteOnly => Access::WriteOnly,
            AccessCfg::ReadWrite => Access::ReadWrite,
        }
    }
}

impl From<EndianCfg> for Endian {
    fn from(e: EndianCfg) -> Self {
        match e {
            EndianCfg::Big => Endian::Big,
            EndianCfg::Little => Endian::Little,
        }
    }
}

impl From<AlignmentCfg> for Alignment {
    fn from(a: AlignmentCfg) -> Self {
        match a {
            AlignmentCfg::Left => Alignment::Left,
            AlignmentCfg::Right => Alignment::Right,
        }
    }
}

impl RegisterDef {
    pub fn kind(&self) -> Kind {
        match self.read_code {
            1 => Kind::Coil,
            2 => Kind::DiscreteInput,
            4 => Kind::HoldingRegister,
            _ => Kind::InputRegister,
        }
    }

    pub fn mem_type(&self) -> Type {
        match self.kind() {
            Kind::Coil | Kind::DiscreteInput => Type::Coil,
            Kind::HoldingRegister | Kind::InputRegister => Type::Register,
        }
    }

    pub fn format(&self) -> Format {
        let res = Resolution(self.resolution);
        let endian: Endian = self.endian.into();
        match self.value_type {
            ValueType::U8 => Format::U8((endian, res)),
            ValueType::U16 => Format::U16((endian, res)),
            ValueType::U32 => Format::U32((endian, res)),
            ValueType::U64 => Format::U64((endian, res)),
            ValueType::U128 => Format::U128((endian, res)),
            ValueType::I8 => Format::I8((endian, res)),
            ValueType::I16 => Format::I16((endian, res)),
            ValueType::I32 => Format::I32((endian, res)),
            ValueType::I64 => Format::I64((endian, res)),
            ValueType::I128 => Format::I128((endian, res)),
            ValueType::F32 => Format::F32((endian, res)),
            ValueType::F64 => Format::F64((endian, res)),
            ValueType::Ascii => Format::Ascii((self.alignment.into(), Width(self.length))),
        }
    }

    pub fn address(&self) -> Address {
        match (self.is_virtual, self.address) {
            (false, Some(addr)) => Address::Fixed(addr),
            _ => Address::Virtual,
        }
    }

    /// Memory range backing this register, or `None` for virtual registers.
    pub fn mem_range(&self) -> Option<Range> {
        match self.address() {
            Address::Fixed(addr) => Some(Range::new(addr as usize, self.format().width())),
            Address::Virtual => None,
        }
    }

    pub fn register(&self) -> Register {
        RegisterBuilder::default()
            .slave_id(self.slave_id)
            .access(self.access.into())
            .kind(self.kind())
            .address(self.address())
            .format(self.format())
            .build()
            .expect("register fields are all set")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_util::convert::{Converter, FileType};

    fn sample() -> DeviceConfig {
        let mut definitions = BTreeMap::new();
        definitions.insert(
            "setpoint".to_string(),
            RegisterDef {
                slave_id: 1,
                read_code: 4,
                address: Some(0),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::U16,
                endian: EndianCfg::Big,
                resolution: 1.0,
                length: 1,
                alignment: AlignmentCfg::Left,
                values: vec![],
                update: None,
                comment: "charge setpoint".to_string(),
            },
        );
        definitions.insert(
            "state".to_string(),
            RegisterDef {
                slave_id: 1,
                read_code: 4,
                address: Some(5),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::I16,
                endian: EndianCfg::Big,
                resolution: 1.0,
                length: 1,
                alignment: AlignmentCfg::Left,
                values: vec![
                    NamedValue {
                        name: "waiting".into(),
                        value: Scalar::Int(0),
                    },
                    NamedValue {
                        name: "charging".into(),
                        value: Scalar::Int(2),
                    },
                    NamedValue {
                        name: "trickle".into(),
                        value: Scalar::Float(1.5),
                    },
                    NamedValue {
                        name: "label".into(),
                        value: Scalar::Text("idle".into()),
                    },
                ],
                update: Some("C_Register:Set(\"power\", C_Register:GetInt(\"setpoint\"))".into()),
                comment: String::new(),
            },
        );
        DeviceConfig {
            version: Some("0.1.0".to_string()),
            comment: "EVSE".to_string(),
            timeout_ms: Some(2000),
            delay_ms: None,
            interval_ms: Some(800),
            read_ranges: ReadRanges {
                holding: Some("0-100,140-160".to_string()),
                ..Default::default()
            },
            definitions,
        }
    }

    fn roundtrip(ty: FileType, ext: &str) {
        let original = sample();
        let path = std::env::temp_dir().join(format!("ferrowl_device_test.{ext}"));
        let path = path.to_str().unwrap();
        Converter::save(&original, path, ty).expect("save");
        let back: DeviceConfig = Converter::load(path, ty).expect("load");
        assert_eq!(original, back);
    }

    #[test]
    fn ut_device_roundtrip_toml() {
        roundtrip(FileType::Toml, "toml");
    }

    #[test]
    fn ut_device_roundtrip_json() {
        roundtrip(FileType::Json, "json");
    }

    #[test]
    fn ut_register_mapping() {
        let def = &sample().definitions["setpoint"];
        assert!(matches!(def.kind(), Kind::HoldingRegister));
        assert!(matches!(def.mem_type(), Type::Register));
        assert_eq!(def.format().width(), 1);
    }
}
