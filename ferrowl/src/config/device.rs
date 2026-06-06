//! Device-type config: the register definitions for one kind of device. One file = one
//! device type (no ip/port/role/name — those are per-instance, set via setup dialog / CLI).

use std::collections::BTreeMap;

use ferrowl_mem::{Range, Type};
use ferrowl_net::FunctionCode;
use ferrowl_reg::{
    Access, Address, Format, Kind, Register, RegisterBuilder,
    format::{Alignment, Endian, Resolution, Width},
};
use ferrowl_ui::traits::ToLabel;
use serde::{Deserialize, Serialize};

/// A device-type configuration file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeviceConfig {
    #[serde(default)]
    pub comment: String,
    pub definitions: BTreeMap<String, RegisterDef>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedValue {
    pub name: String,
    pub value: i64,
}

impl ToLabel for NamedValue {
    fn to_label(&self) -> String {
        self.name.clone()
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

    pub fn function_code(&self) -> FunctionCode {
        match self.kind() {
            Kind::Coil => FunctionCode::ReadCoils,
            Kind::DiscreteInput => FunctionCode::ReadDiscreteInputs,
            Kind::HoldingRegister => FunctionCode::ReadHoldingRegisters,
            Kind::InputRegister => FunctionCode::ReadInputRegisters,
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
                        value: 0,
                    },
                    NamedValue {
                        name: "charging".into(),
                        value: 2,
                    },
                ],
                update: Some("C_Register:Set(\"power\", C_Register:GetInt(\"setpoint\"))".into()),
                comment: String::new(),
            },
        );
        DeviceConfig {
            comment: "EVSE".to_string(),
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
