//! Device-type config: the register definitions for one kind of device. One file = one
//! device type (no ip/port/role/name — those are per-instance, set via setup dialog / CLI).

use std::collections::BTreeMap;

use ferrowl_codec::{
    Access, Address, Format, Kind, Register, RegisterBuilder,
    format::{Alignment, BitField, Endian, Resolution, Width},
};
use ferrowl_store::{CellType, Range};
use ferrowl_ui::traits::ToLabel;
use serde::{Deserialize, Serialize};

/// Fallback timing (ms) when neither the module spec nor the device config sets a value.
pub const DEFAULT_TIMEOUT_MS: usize = 3000;
pub const DEFAULT_DELAY_MS: usize = 1000;
pub const DEFAULT_INTERVAL_MS: usize = 1000;

/// A device-type configuration file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeviceConfig {
    /// Ferrowl version that wrote this file, stamped on save. Enables future compatibility
    /// shims when loading configs produced by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Device-level timing defaults (ms). Used when a `ModuleSpec` does not override them; the
    /// built-in defaults (`DEFAULT_*_MS`) are the final fallback. See `Module::resolve_timing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<usize>,
    /// Base path for per-module log files (tab name appended as suffix). `None` disables.
    #[serde(skip)]
    pub log_file: Option<String>,
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
    #[serde(default = "default_kind")]
    pub kind: Kind,
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
    /// Optional bit-field mask for integer types, as a hex (`"0xFF00"`) or decimal
    /// string. The shift is derived from the mask's trailing zeros. Omitted ⇒ the
    /// full value (no masking). Ignored for float and ASCII types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitmask: Option<String>,
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
    pub description: String,
    /// Default value written to memory on configuration load / startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Scalar>,
}

/// A human-readable name for one specific register value (enum-like state).
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
///
/// Serialization is derived untagged. Deserialization is hand-written via a `Visitor` rather than
/// `#[serde(untagged)]` because that buffers content into an intermediate that `serde_json`'s
/// `arbitrary_precision` mode represents numbers as — making untagged number variants fail to
/// match. A direct visitor avoids the buffering and also accepts the `arbitrary_precision` number
/// wrapper, so it works under any serde_json feature set as well as for TOML.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Scalar {
    Int(i64),
    Float(f64),
    Text(String),
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ScalarVisitor;

        impl<'de> serde::de::Visitor<'de> for ScalarVisitor {
            type Value = Scalar;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an integer, float, or string")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Scalar, E> {
                Ok(Scalar::Int(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Scalar, E> {
                Ok(i64::try_from(v).map_or(Scalar::Float(v as f64), Scalar::Int))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Scalar, E> {
                Ok(Scalar::Float(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Scalar, E> {
                Ok(Scalar::Text(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Scalar, E> {
                Ok(Scalar::Text(v))
            }

            // `serde_json`'s `arbitrary_precision` mode (enabled transitively by `rust-ocpp`'s
            // `jsonschema`/`rust_decimal` deps when this crate shares a workspace build) encodes a
            // number as a single-entry map `{ "$serde_json::private::Number": "<digits>" }`. Parse
            // that back into the appropriate variant.
            fn visit_map<A>(self, mut map: A) -> Result<Scalar, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let _key: String = map
                    .next_key()?
                    .ok_or_else(|| serde::de::Error::custom("empty number map"))?;
                let raw: String = map.next_value()?;
                if let Ok(i) = raw.parse::<i64>() {
                    Ok(Scalar::Int(i))
                } else if let Ok(f) = raw.parse::<f64>() {
                    Ok(Scalar::Float(f))
                } else {
                    Err(serde::de::Error::custom(format!("invalid number: {raw}")))
                }
            }
        }

        deserializer.deserialize_any(ScalarVisitor)
    }
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

    pub fn to_value(&self, res: f64) -> ferrowl_codec::Value {
        match self {
            Scalar::Int(i) => ferrowl_codec::Value::I64((*i, Resolution(res))),
            Scalar::Float(f) => ferrowl_codec::Value::F64((*f, Resolution(res))),
            Scalar::Text(s) => ferrowl_codec::Value::Ascii(s.clone()),
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

/// Config-file spelling of register access rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AccessCfg {
    ReadOnly,
    WriteOnly,
    #[default]
    ReadWrite,
}

/// Config-file spelling of byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum EndianCfg {
    #[default]
    Big,
    Little,
}

/// Config-file spelling of ASCII alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AlignmentCfg {
    #[default]
    Left,
    Right,
}

/// Parse a config `bitmask` string into a [`BitField`]. Accepts `0x`-prefixed hex
/// or decimal; `None`, empty, or an unparseable value yields the full mask (no-op).
pub fn parse_bitmask(s: Option<&str>) -> BitField {
    let mask = match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => u128::MAX,
        Some(s) => s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .map(|hex| u128::from_str_radix(hex, 16))
            .unwrap_or_else(|| s.parse::<u128>())
            .unwrap_or(u128::MAX),
    };
    BitField { mask }
}

fn default_kind() -> Kind {
    Kind::InputRegister
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
        self.kind.clone()
    }

    pub fn mem_type(&self) -> CellType {
        match self.kind() {
            Kind::Coil | Kind::DiscreteInput => CellType::Coil,
            Kind::HoldingRegister | Kind::InputRegister => CellType::Register,
        }
    }

    /// The configured bit-field for integer types: parses [`bitmask`](Self::bitmask)
    /// (hex `0x…` or decimal) into a [`BitField`], defaulting to the full mask when
    /// absent or unparseable.
    pub fn bitfield(&self) -> BitField {
        parse_bitmask(self.bitmask.as_deref())
    }

    pub fn format(&self) -> Format {
        let res = Resolution(self.resolution);
        let endian: Endian = self.endian.into();
        let bf = self.bitfield();
        match self.value_type {
            ValueType::U8 => Format::U8((endian, res, bf)),
            ValueType::U16 => Format::U16((endian, res, bf)),
            ValueType::U32 => Format::U32((endian, res, bf)),
            ValueType::U64 => Format::U64((endian, res, bf)),
            ValueType::U128 => Format::U128((endian, res, bf)),
            ValueType::I8 => Format::I8((endian, res, bf)),
            ValueType::I16 => Format::I16((endian, res, bf)),
            ValueType::I32 => Format::I32((endian, res, bf)),
            ValueType::I64 => Format::I64((endian, res, bf)),
            ValueType::I128 => Format::I128((endian, res, bf)),
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
                kind: Kind::HoldingRegister,
                address: Some(0),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::U16,
                endian: EndianCfg::Big,
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: AlignmentCfg::Left,
                values: vec![],
                update: None,
                description: "charge setpoint".to_string(),
                default: None,
            },
        );
        definitions.insert(
            "state".to_string(),
            RegisterDef {
                slave_id: 1,
                kind: Kind::HoldingRegister,
                address: Some(5),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::I16,
                endian: EndianCfg::Big,
                resolution: 1.0,
                bitmask: None,
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
                description: String::new(),
                default: None,
            },
        );
        DeviceConfig {
            version: Some("0.1.0".to_string()),
            timeout_ms: Some(2000),
            delay_ms: None,
            interval_ms: Some(800),
            // `log_file` is `#[serde(skip)]` (runtime-only), so it never survives a
            // config roundtrip — leave it None to match the loaded value.
            log_file: None,
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
        assert!(matches!(def.mem_type(), CellType::Register));
        assert_eq!(def.format().width(), 1);
    }

    #[test]
    fn ut_parse_ranges() {
        // Inclusive bounds; bare address = single-cell range; whitespace tolerated.
        assert_eq!(
            parse_ranges("0-100, 140-160 ,5"),
            vec![Range::new(0, 101), Range::new(140, 21), Range::new(5, 1)]
        );
        // Malformed, reversed and empty entries are skipped.
        assert_eq!(parse_ranges("abc,10-x,,9-3"), vec![]);
        assert_eq!(parse_ranges(""), vec![]);
        // Reversed bound dropped, valid neighbor kept.
        assert_eq!(parse_ranges("9-3,4"), vec![Range::new(4, 1)]);
    }

    #[test]
    fn ut_parse_bitmask() {
        assert_eq!(parse_bitmask(Some("0xFF00")).mask, 0xFF00);
        assert_eq!(parse_bitmask(Some("0xFF00")).shift(), 8);
        assert_eq!(parse_bitmask(Some("65280")).mask, 65280);
        // Absent, empty, or garbage → full mask (no-op).
        assert!(parse_bitmask(None).is_full());
        assert!(parse_bitmask(Some("   ")).is_full());
        assert!(parse_bitmask(Some("nonsense")).is_full());
    }

    #[test]
    fn ut_bitmask_threaded_into_format() {
        let mut def = sample().definitions["setpoint"].clone();
        def.bitmask = Some("0x0FF0".to_string());
        assert_eq!(def.format().bitfield().mask, 0x0FF0);
        // Float types ignore the bitmask (full default).
        def.value_type = ValueType::F32;
        assert!(def.format().bitfield().is_full());
    }

    fn def_with(
        value_type: ValueType,
        kind: Kind,
        address: Option<u16>,
        is_virtual: bool,
    ) -> RegisterDef {
        RegisterDef {
            slave_id: 1,
            kind,
            address,
            is_virtual,
            access: AccessCfg::ReadWrite,
            value_type,
            endian: EndianCfg::Little,
            resolution: 1.0,
            bitmask: None,
            length: 4,
            alignment: AlignmentCfg::Right,
            values: vec![],
            update: None,
            description: String::new(),
            default: None,
        }
    }

    #[test]
    fn ut_read_ranges_is_empty_and_ranges_for() {
        let mut rr = ReadRanges::default();
        assert!(rr.is_empty());
        rr.holding = Some("1".into());
        rr.input = Some("0-3".into());
        rr.coils = Some("5".into());
        rr.discrete = Some("7-8".into());
        assert!(!rr.is_empty());
        assert_eq!(rr.ranges_for(Kind::HoldingRegister), vec![Range::new(1, 1)]);
        assert_eq!(rr.ranges_for(Kind::InputRegister), vec![Range::new(0, 4)]);
        assert_eq!(rr.ranges_for(Kind::Coil), vec![Range::new(5, 1)]);
        assert_eq!(rr.ranges_for(Kind::DiscreteInput), vec![Range::new(7, 2)]);
        // Unconfigured kind -> empty.
        assert!(ReadRanges::default().ranges_for(Kind::Coil).is_empty());
    }

    #[test]
    fn ut_scalar_and_named_value() {
        assert_eq!(Scalar::Int(5).to_string(), "5");
        assert_eq!(Scalar::Float(1.5).to_string(), "1.5");
        assert_eq!(Scalar::Text("hi".into()).to_string(), "hi");
        assert!(matches!(Scalar::from_input(" 7 "), Scalar::Int(7)));
        assert!(matches!(Scalar::from_input("2.5"), Scalar::Float(_)));
        assert!(matches!(Scalar::from_input("abc"), Scalar::Text(_)));
        assert!(matches!(
            Scalar::Int(1).to_value(1.0),
            ferrowl_codec::Value::I64(_)
        ));
        assert!(matches!(
            Scalar::Float(1.0).to_value(1.0),
            ferrowl_codec::Value::F64(_)
        ));
        assert!(matches!(
            Scalar::Text("x".into()).to_value(1.0),
            ferrowl_codec::Value::Ascii(_)
        ));
        let nv = NamedValue {
            name: "n".into(),
            value: Scalar::Int(0),
        };
        assert_eq!(nv.to_label(), "n");
    }

    #[test]
    fn ut_cfg_conversions() {
        assert!(matches!(
            Access::from(AccessCfg::ReadOnly),
            Access::ReadOnly
        ));
        assert!(matches!(
            Access::from(AccessCfg::WriteOnly),
            Access::WriteOnly
        ));
        assert!(matches!(
            Access::from(AccessCfg::ReadWrite),
            Access::ReadWrite
        ));
        assert!(matches!(Endian::from(EndianCfg::Big), Endian::Big));
        assert!(matches!(Endian::from(EndianCfg::Little), Endian::Little));
        assert!(matches!(
            Alignment::from(AlignmentCfg::Left),
            Alignment::Left
        ));
        assert!(matches!(
            Alignment::from(AlignmentCfg::Right),
            Alignment::Right
        ));
    }

    #[test]
    fn ut_register_def_kind_mem_type_and_all_formats() {
        for kind in [
            Kind::Coil,
            Kind::DiscreteInput,
            Kind::InputRegister,
            Kind::HoldingRegister,
        ] {
            assert_eq!(
                def_with(ValueType::U16, kind.clone(), Some(0), false).kind(),
                kind
            );
        }
        assert_eq!(
            def_with(ValueType::U16, Kind::Coil, Some(0), false).mem_type(),
            CellType::Coil
        );
        assert_eq!(
            def_with(ValueType::U16, Kind::HoldingRegister, Some(0), false).mem_type(),
            CellType::Register
        );

        for vt in [
            ValueType::U8,
            ValueType::U16,
            ValueType::U32,
            ValueType::U64,
            ValueType::U128,
            ValueType::I8,
            ValueType::I16,
            ValueType::I32,
            ValueType::I64,
            ValueType::I128,
            ValueType::F32,
            ValueType::F64,
            ValueType::Ascii,
        ] {
            let _ = def_with(vt, Kind::HoldingRegister, Some(0), false).format();
        }
    }

    #[test]
    fn ut_register_def_address_range_and_register() {
        let fixed = def_with(ValueType::U16, Kind::HoldingRegister, Some(10), false);
        assert_eq!(fixed.address(), Address::Fixed(10));
        assert!(fixed.mem_range().is_some());
        let _ = fixed.register();
        let _ = fixed.bitfield();

        let virt = def_with(ValueType::U16, Kind::HoldingRegister, None, false);
        assert_eq!(virt.address(), Address::Virtual);
        assert!(virt.mem_range().is_none());

        // The `virtual` flag forces Virtual even with a concrete address.
        assert_eq!(
            def_with(ValueType::U16, Kind::HoldingRegister, Some(5), true).address(),
            Address::Virtual
        );
    }

    #[test]
    fn ut_register_def_serde_defaults() {
        // A minimal definition omitting kind/resolution/length triggers the default fns.
        let path = std::env::temp_dir().join("ferrowl_codecdef_min.toml");
        let path = path.to_str().unwrap();
        std::fs::write(path, "type = \"U16\"\n").unwrap();
        let def: RegisterDef = Converter::load(path, FileType::Toml).unwrap();
        assert_eq!(def.kind, Kind::InputRegister);
        assert_eq!(def.resolution, 1.0);
        assert_eq!(def.length, 1);
    }
}
