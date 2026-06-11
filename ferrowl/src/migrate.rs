//! Migration from the pre-rewrite (≤ 0.3.9) `modbus-cli-rs` configuration to the current
//! `DeviceConfig`.
//!
//! The old format was a single flat file that bundled timing globals, contiguous memory ranges,
//! and register definitions together.  The new format separates concerns: timing lives in the
//! app config or per-module spec, while a device-type file contains only `read_ranges` and
//! `definitions`.
//!
//! Notable format differences:
//!
//! | Aspect              | v0.3.9                       | current                             |
//! |---------------------|------------------------------|-------------------------------------|
//! | Holding read code   | `read_code = 3`              | `read_code = 4`                     |
//! | Input read code     | `read_code = 4`              | `read_code = 3`                     |
//! | Little-endian types | `type = "U32le"`             | `type = "U32"`, `endian = "Little"` |
//! | Lua hook field      | `on_update`                  | `update`                            |
//! | Contiguous ranges   | `[[contiguous_memory]]`      | `[read_ranges]`                     |
//! | Connect delay       | `delay_after_connect_ms`     | `delay_ms`                          |
//!
//! Fields with no device-config equivalent (`history_length`, `reverse`) are dropped; a warning
//! is emitted for each one. Per-register `default` is now preserved.

use std::collections::BTreeMap;

use ferrowl_util::convert::{Converter, FileType};
use serde::Deserialize;

use crate::cli::MigrateArgs;
use crate::config::device::{
    AccessCfg, AlignmentCfg, DeviceConfig, EndianCfg, NamedValue, ReadRanges, RegisterDef, Scalar,
    ValueType,
};

// ── Legacy structure definitions ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LegacyConfig {
    history_length: Option<u32>,
    interval_ms: Option<u64>,
    delay_after_connect_ms: Option<u64>,
    timeout_ms: Option<u64>,
    #[serde(default)]
    contiguous_memory: Vec<LegacyMemoryRange>,
    #[serde(default)]
    definitions: BTreeMap<String, LegacyRegisterDef>,
}

#[derive(Debug, Deserialize)]
struct LegacyMemoryRange {
    #[serde(default)]
    slave_id: u8,
    read_code: u8,
    range: LegacyRange,
}

#[derive(Debug, Deserialize)]
struct LegacyRange {
    start: LegacyAddress,
    end: LegacyAddress,
}

/// An address that may be written as a decimal integer or a `"0xHEX"` string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LegacyAddress {
    Int(u32),
    Str(String),
}

impl LegacyAddress {
    fn resolve(&self) -> Result<u32, String> {
        match self {
            LegacyAddress::Int(n) => Ok(*n),
            LegacyAddress::Str(s) => {
                let hex = s.trim().trim_start_matches("0x").trim_start_matches("0X");
                u32::from_str_radix(hex, 16).map_err(|_| format!("invalid address '{s}'"))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct LegacyRegisterDef {
    #[serde(default)]
    slave_id: u8,
    #[serde(default = "legacy_default_read_code")]
    read_code: u8,
    address: Option<LegacyAddress>,
    #[serde(default = "legacy_default_length")]
    length: usize,
    #[serde(default = "legacy_default_access")]
    access: String,
    #[serde(rename = "type")]
    value_type: String,
    #[serde(default)]
    reverse: bool,
    #[serde(default = "legacy_default_resolution")]
    resolution: f64,
    #[serde(rename = "virtual", default)]
    is_virtual: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    on_update: Option<String>,
    #[serde(default)]
    values: Vec<LegacyValue>,
    #[serde(default)]
    default: Option<LegacyScalar>,
}

fn legacy_default_read_code() -> u8 {
    3
}
fn legacy_default_length() -> usize {
    1
}
fn legacy_default_access() -> String {
    "ReadWrite".into()
}
fn legacy_default_resolution() -> f64 {
    1.0
}

/// A value entry in the old `values` list: either `{ name = "…", value = N }` or a bare integer.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LegacyValue {
    Named { name: String, value: LegacyScalar },
    Bare(i64),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LegacyScalar {
    Int(i64),
    Float(f64),
    Text(String),
}

impl From<LegacyScalar> for Scalar {
    fn from(s: LegacyScalar) -> Self {
        match s {
            LegacyScalar::Int(n) => Scalar::Int(n),
            LegacyScalar::Float(f) => Scalar::Float(f),
            LegacyScalar::Text(t) => Scalar::Text(t),
        }
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

/// Remap the read code from the old convention to the new one.
///
/// The old format followed the Modbus standard (FC03 = Holding, FC04 = Input).
/// The current format swaps those two codes (4 = Holding, 3 = Input).
/// Coil (1) and DiscreteInput (2) are unchanged.
fn migrate_read_code(old: u8) -> u8 {
    match old {
        3 => 4,
        4 => 3,
        other => other,
    }
}

/// Parse an old type string into a `(ValueType, EndianCfg)` pair.
///
/// Little-endian variants in the old format used an `"le"` suffix (e.g. `"U32le"`).
/// UTF-8 text types (`PackedUtf8`, `LooseUtf8`) have no equivalent and are mapped to `Ascii`.
fn parse_type(s: &str) -> Result<(ValueType, EndianCfg), String> {
    let (base, endian) = match s.strip_suffix("le") {
        Some(b) => (b, EndianCfg::Little),
        None => (s, EndianCfg::Big),
    };

    let vt = match base {
        "U8" => ValueType::U8,
        "U16" => ValueType::U16,
        "U32" => ValueType::U32,
        "U64" => ValueType::U64,
        "U128" => ValueType::U128,
        "I8" => ValueType::I8,
        "I16" => ValueType::I16,
        "I32" => ValueType::I32,
        "I64" => ValueType::I64,
        "I128" => ValueType::I128,
        "F32" => ValueType::F32,
        "F64" => ValueType::F64,
        "PackedAscii" | "LooseAscii" | "PackedUtf8" | "LooseUtf8" => ValueType::Ascii,
        other => return Err(format!("unknown type '{other}'")),
    };
    Ok((vt, endian))
}

fn parse_access(s: &str) -> Result<AccessCfg, String> {
    match s {
        "ReadOnly" => Ok(AccessCfg::ReadOnly),
        "WriteOnly" => Ok(AccessCfg::WriteOnly),
        "ReadWrite" => Ok(AccessCfg::ReadWrite),
        other => Err(format!("unknown access '{other}'")),
    }
}

/// Convert `[[contiguous_memory]]` entries into the new `[read_ranges]` structure.
///
/// The old format kept per-slave ranges; the new format only distinguishes function codes, so
/// all ranges for the same code are merged into a single comma-separated string.
fn convert_ranges(src: &[LegacyMemoryRange], warnings: &mut Vec<String>) -> ReadRanges {
    let mut holding: Vec<String> = Vec::new();
    let mut input: Vec<String> = Vec::new();
    let mut coils: Vec<String> = Vec::new();
    let mut discrete: Vec<String> = Vec::new();

    for mem in src {
        let start = match mem.range.start.resolve() {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("contiguous_memory: {e}; entry skipped"));
                continue;
            }
        };
        let end = match mem.range.end.resolve() {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("contiguous_memory: {e}; entry skipped"));
                continue;
            }
        };
        // slave_id has no equivalent in read_ranges; warn when non-zero.
        if mem.slave_id != 0 {
            warnings.push(format!(
                "contiguous_memory read_code={} slave_id={}: \
                 slave_id is not supported in read_ranges and was dropped",
                mem.read_code, mem.slave_id
            ));
        }

        let entry = if start == end {
            format!("{start}")
        } else {
            format!("{start}-{end}")
        };

        // Old convention: 3=Holding, 4=Input.
        match mem.read_code {
            1 => coils.push(entry),
            2 => discrete.push(entry),
            3 => holding.push(entry),
            4 => input.push(entry),
            other => warnings.push(format!(
                "contiguous_memory: unknown read_code {other}; entry skipped"
            )),
        }
    }

    let join = |v: Vec<String>| {
        if v.is_empty() {
            None
        } else {
            Some(v.join(","))
        }
    };
    ReadRanges {
        holding: join(holding),
        input: join(input),
        coils: join(coils),
        discrete: join(discrete),
    }
}

fn convert_values(
    src: Vec<LegacyValue>,
    reg_name: &str,
    warnings: &mut Vec<String>,
) -> Vec<NamedValue> {
    src.into_iter()
        .enumerate()
        .map(|(i, v)| match v {
            LegacyValue::Named { name, value } => NamedValue {
                name,
                value: value.into(),
            },
            LegacyValue::Bare(n) => {
                warnings.push(format!(
                    "'{reg_name}': values[{i}] is a bare integer {n}; \
                     used \"{n}\" as its display name"
                ));
                NamedValue {
                    name: n.to_string(),
                    value: Scalar::Int(n),
                }
            }
        })
        .collect()
}

fn convert_def(
    reg_name: &str,
    src: LegacyRegisterDef,
    warnings: &mut Vec<String>,
) -> Result<RegisterDef, String> {
    let (value_type, endian) = parse_type(&src.value_type)?;

    if src.value_type.contains("Utf8") {
        warnings.push(format!(
            "'{reg_name}': type '{}' mapped to Ascii — \
             UTF-8 string encoding is not supported in the current format",
            src.value_type
        ));
    }
    if src.reverse {
        warnings.push(format!(
            "'{reg_name}': 'reverse' (byte-swap within each register) \
             has no equivalent in the current format and was dropped"
        ));
    }
    let access = parse_access(&src.access).map_err(|e| format!("access: {e}"))?;

    let address = match src.address {
        None => None,
        Some(a) => {
            let raw = a.resolve()?;
            if raw > u16::MAX as u32 {
                return Err(format!(
                    "address 0x{raw:04x} exceeds the 16-bit register range"
                ));
            }
            Some(raw as u16)
        }
    };

    let values = convert_values(src.values, reg_name, warnings);
    let read_code = migrate_read_code(src.read_code);

    Ok(RegisterDef {
        slave_id: src.slave_id,
        read_code,
        address,
        is_virtual: src.is_virtual,
        access,
        value_type,
        endian,
        resolution: src.resolution,
        length: src.length,
        alignment: AlignmentCfg::Left,
        values,
        update: src.on_update,
        description: src.description,
        default: src.default.map(Scalar::from),
    })
}

fn convert(legacy: LegacyConfig) -> (DeviceConfig, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();

    if legacy.history_length.is_some() {
        warnings.push(
            "'history_length' has no equivalent in device config; dropped".into(),
        );
    }

    let read_ranges = convert_ranges(&legacy.contiguous_memory, &mut warnings);

    let mut definitions = BTreeMap::new();
    for (name, def) in legacy.definitions {
        match convert_def(&name, def, &mut warnings) {
            Ok(d) => {
                definitions.insert(name, d);
            }
            Err(e) => warnings.push(format!("skipping '{name}': {e}")),
        }
    }

    let device = DeviceConfig {
        version: Some(crate::config::VERSION.to_string()),
        description: String::new(),
        timeout_ms: legacy.timeout_ms.map(|v| v as usize),
        delay_ms: legacy.delay_after_connect_ms.map(|v| v as usize),
        interval_ms: legacy.interval_ms.map(|v| v as usize),
        log_file: None,
        read_ranges,
        definitions,
    };

    (device, warnings)
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run(args: &MigrateArgs) {
    let input_type = match FileType::from_path(&args.input) {
        Some(t) => t,
        None => {
            eprintln!(
                "error: cannot infer input format from '{}' (expected .toml or .json)",
                args.input
            );
            std::process::exit(1);
        }
    };

    let legacy: LegacyConfig = match Converter::load(&args.input, input_type) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to parse '{}': {:?}", args.input, e);
            std::process::exit(1);
        }
    };

    let (device, warnings) = convert(legacy);

    for w in &warnings {
        eprintln!("warning: {w}");
    }

    let output_type = match FileType::from_path(&args.output) {
        Some(t) => t,
        None => {
            eprintln!(
                "error: cannot infer output format from '{}' (expected .toml or .json)",
                args.output
            );
            std::process::exit(1);
        }
    };

    match Converter::save(&device, &args.output, output_type) {
        Ok(()) => eprintln!("Migrated device config written to '{}'.", args.output),
        Err(e) => {
            eprintln!("error: failed to write '{}': {:?}", args.output, e);
            std::process::exit(1);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_sample() -> LegacyConfig {
        let mut definitions = BTreeMap::new();
        definitions.insert(
            "setpoint_rpm".into(),
            LegacyRegisterDef {
                slave_id: 1,
                read_code: 3, // old Holding
                address: Some(LegacyAddress::Str("0x1000".into())),
                length: 2,
                access: "ReadWrite".into(),
                value_type: "U32".into(),
                reverse: false,
                resolution: 10.0,
                is_virtual: false,
                description: "RPM setpoint".into(),
                on_update: None,
                values: vec![],
                default: Some(LegacyScalar::Int(1500)),
            },
        );
        definitions.insert(
            "temperature_f32le".into(),
            LegacyRegisterDef {
                slave_id: 1,
                read_code: 4, // old Input
                address: Some(LegacyAddress::Str("0x2002".into())),
                length: 2,
                access: "ReadOnly".into(),
                value_type: "F32le".into(),
                reverse: false,
                resolution: 1.0,
                is_virtual: false,
                description: "Temperature LE".into(),
                on_update: None,
                values: vec![],
                default: None,
            },
        );
        definitions.insert(
            "evse_state".into(),
            LegacyRegisterDef {
                slave_id: 1,
                read_code: 4,
                address: Some(LegacyAddress::Str("0x3000".into())),
                length: 1,
                access: "ReadOnly".into(),
                value_type: "I16".into(),
                reverse: false,
                resolution: 1.0,
                is_virtual: false,
                description: "EVSE state".into(),
                on_update: None,
                values: vec![
                    LegacyValue::Named {
                        name: "waiting".into(),
                        value: LegacyScalar::Int(0),
                    },
                    LegacyValue::Bare(255),
                ],
                default: None,
            },
        );
        definitions.insert(
            "status_label".into(),
            LegacyRegisterDef {
                slave_id: 1,
                read_code: 4,
                address: Some(LegacyAddress::Str("0x4000".into())),
                length: 10,
                access: "ReadWrite".into(),
                value_type: "PackedAscii".into(),
                reverse: false,
                resolution: 1.0,
                is_virtual: true,
                description: "Status string".into(),
                on_update: Some("C_Register:Set(\"status_label\", \"ok\")".into()),
                values: vec![],
                default: None,
            },
        );
        LegacyConfig {
            history_length: Some(50),
            interval_ms: Some(500),
            delay_after_connect_ms: Some(500),
            timeout_ms: Some(3000),
            contiguous_memory: vec![
                LegacyMemoryRange {
                    slave_id: 1,
                    read_code: 4, // old Input
                    range: LegacyRange {
                        start: LegacyAddress::Str("0x0000".into()),
                        end: LegacyAddress::Str("0x000F".into()),
                    },
                },
                LegacyMemoryRange {
                    slave_id: 0,
                    read_code: 3, // old Holding
                    range: LegacyRange {
                        start: LegacyAddress::Int(100),
                        end: LegacyAddress::Int(200),
                    },
                },
            ],
            definitions,
        }
    }

    #[test]
    fn ut_migrate_read_code_swap() {
        assert_eq!(migrate_read_code(3), 4);
        assert_eq!(migrate_read_code(4), 3);
        assert_eq!(migrate_read_code(1), 1);
        assert_eq!(migrate_read_code(2), 2);
    }

    #[test]
    fn ut_parse_le_type() {
        let (vt, endian) = parse_type("F32le").unwrap();
        assert_eq!(vt, ValueType::F32);
        assert_eq!(endian, EndianCfg::Little);

        let (vt, endian) = parse_type("U32").unwrap();
        assert_eq!(vt, ValueType::U32);
        assert_eq!(endian, EndianCfg::Big);
    }

    #[test]
    fn ut_parse_ascii_types() {
        for t in &["PackedAscii", "LooseAscii", "PackedUtf8", "LooseUtf8"] {
            let (vt, _) = parse_type(t).unwrap();
            assert_eq!(vt, ValueType::Ascii);
        }
    }

    #[test]
    fn ut_address_hex_parse() {
        assert_eq!(
            LegacyAddress::Str("0x1000".into()).resolve().unwrap(),
            0x1000
        );
        assert_eq!(LegacyAddress::Str("0X00FF".into()).resolve().unwrap(), 0xFF);
        assert_eq!(LegacyAddress::Int(42).resolve().unwrap(), 42);
    }

    #[test]
    fn ut_bare_value_gets_integer_name() {
        let mut warnings = Vec::new();
        let values = convert_values(
            vec![
                LegacyValue::Named {
                    name: "ok".into(),
                    value: LegacyScalar::Int(0),
                },
                LegacyValue::Bare(255),
            ],
            "reg",
            &mut warnings,
        );
        assert_eq!(values.len(), 2);
        assert_eq!(values[1].name, "255");
        assert!(matches!(values[1].value, Scalar::Int(255)));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn ut_convert_full_sample() {
        let (device, warnings) = convert(legacy_sample());

        // history_length drop warning
        assert!(warnings.iter().any(|w| w.contains("history_length")));
        // bare value warning
        assert!(warnings.iter().any(|w| w.contains("255")));
        // slave_id warning for non-zero slave in contiguous_memory
        assert!(warnings.iter().any(|w| w.contains("slave_id")));

        // timing fields preserved
        assert_eq!(device.interval_ms, Some(500));
        assert_eq!(device.timeout_ms, Some(3000));
        assert_eq!(device.delay_ms, Some(500));

        // read_ranges: old read_code=3 (Holding) → read_ranges.holding
        //              old read_code=4 (Input)   → read_ranges.input
        assert_eq!(device.read_ranges.holding, Some("100-200".into()));
        assert_eq!(device.read_ranges.input, Some("0-15".into()));

        // setpoint_rpm: old read_code=3 (Holding) → new read_code=4; default preserved
        let rpm = &device.definitions["setpoint_rpm"];
        assert_eq!(rpm.read_code, 4);
        assert_eq!(rpm.value_type, ValueType::U32);
        assert_eq!(rpm.endian, EndianCfg::Big);
        assert_eq!(rpm.address, Some(0x1000));
        assert!(matches!(rpm.default, Some(Scalar::Int(1500))));

        // temperature_f32le: old read_code=4 (Input) → new read_code=3; endian=Little
        let temp = &device.definitions["temperature_f32le"];
        assert_eq!(temp.read_code, 3);
        assert_eq!(temp.value_type, ValueType::F32);
        assert_eq!(temp.endian, EndianCfg::Little);

        // on_update → update
        let label = &device.definitions["status_label"];
        assert!(label.update.is_some());
        assert_eq!(label.value_type, ValueType::Ascii);
        assert!(label.is_virtual);
    }
}
