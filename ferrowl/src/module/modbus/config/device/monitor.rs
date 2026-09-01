//! api-contract.md §6/§6.3 — the role-conditional device config shape for `role = monitor`:
//! `MonitorDeviceConfig` (a subset of `DeviceConfig`, no timing/read_ranges/scripts) and
//! `MonitorRegisterDef` (MB-R-145; `RegisterDef` minus `access` and `update`).

use ferrowl_codec::{
    Address, Format, Kind,
    format::{Endian, Resolution, Width, WordOrder},
};
use serde::{Deserialize, Serialize};

use super::{
    AlignmentCfg, EndianCfg, NamedValue, Scalar, ValueType, WordOrderCfg, default_kind,
    default_length, default_resolution, parse_bitmask,
};

/// A monitor device-type configuration file (api-contract.md §6, role-conditional shape):
/// carries only `version`, `reconnect` (also gates MB-R-141's serial-open retry), and
/// `definitions` — no timing (`timeout_ms`/`delay_ms`/`interval_ms`), no `read_ranges`, no Lua
/// sim surface (`scripts`/`script_interval`): a monitor never initiates a transaction, has no
/// poll loop, and is display-only.
// `#[allow(dead_code)]` covers the not-yet-constructed members only; the schema and its
// `.format()`/`.address()` methods are implemented and tested here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[allow(dead_code)]
pub struct MonitorDeviceConfig {
    /// Ferrowl version that wrote this file, stamped on save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Gates MB-R-141's serial-open retry. `None` falls back to
    /// [`super::DEFAULT_RECONNECT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect: Option<bool>,
    /// Base path for per-module log files (tab name appended as suffix). `None` disables.
    /// Mirrors `DeviceConfig::log_file`'s own `#[serde(skip)]` field — never written to disk.
    #[serde(skip)]
    pub log_file: Option<String>,
    /// api-contract.md §6: a list, not a name-keyed map — two interpretations on different
    /// `slave_id`s may legitimately share a `name` (MB-R-148 scopes edit/remove to one slave
    /// id's set), and a name-keyed map would silently collapse them on save.
    #[serde(default)]
    pub definitions: Vec<MonitorRegisterDef>,
}

/// MB-R-145 — a display-only register interpretation against a monitor's observed-value table:
/// every field [`super::RegisterDef`] has, verbatim, except `access` (no access direction — the
/// table is observed, not owned) and `update` (no store cell to script against). Also carries its
/// own `name`, since `definitions` is a list rather than a name-keyed map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(dead_code)] // not constructed yet, see `MonitorDeviceConfig`'s note
pub struct MonitorRegisterDef {
    pub name: String,
    #[serde(default)]
    pub slave_id: u8,
    #[serde(default = "default_kind")]
    pub kind: Kind,
    #[serde(default)]
    pub address: Option<u16>,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    #[serde(rename = "type")]
    pub value_type: ValueType,
    #[serde(default)]
    pub endian: EndianCfg,
    #[serde(default)]
    pub word_order: WordOrderCfg,
    #[serde(default = "default_resolution")]
    pub resolution: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitmask: Option<String>,
    #[serde(default = "default_length")]
    pub length: usize,
    #[serde(default)]
    pub alignment: AlignmentCfg,
    #[serde(default)]
    pub values: Vec<NamedValue>,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Scalar>,
}

#[allow(dead_code)] // not constructed yet, see `MonitorDeviceConfig`'s note
impl MonitorRegisterDef {
    /// Identical body to [`super::RegisterDef::format`] — same format machinery, MB-R-145.
    pub fn format(&self) -> Format {
        let res = Resolution(self.resolution);
        let endian: Endian = self.endian.into();
        let wo: WordOrder = self.word_order.into();
        let bf = parse_bitmask(self.bitmask.as_deref());
        match self.value_type {
            ValueType::U8 => Format::u8(endian, wo, res, bf),
            ValueType::U16 => Format::u16(endian, wo, res, bf),
            ValueType::U32 => Format::u32(endian, wo, res, bf),
            ValueType::U64 => Format::u64(endian, wo, res, bf),
            ValueType::U128 => Format::u128(endian, wo, res, bf),
            ValueType::I8 => Format::i8(endian, wo, res, bf),
            ValueType::I16 => Format::i16(endian, wo, res, bf),
            ValueType::I32 => Format::i32(endian, wo, res, bf),
            ValueType::I64 => Format::i64(endian, wo, res, bf),
            ValueType::I128 => Format::i128(endian, wo, res, bf),
            ValueType::F32 => Format::f32(endian, wo, res),
            ValueType::F64 => Format::f64(endian, wo, res),
            ValueType::Ascii => Format::Ascii(self.alignment.into(), Width(self.length)),
        }
    }

    /// Identical body to [`super::RegisterDef::address`].
    pub fn address(&self) -> Address {
        match (self.is_virtual, self.address) {
            (false, Some(addr)) => Address::Fixed(addr),
            _ => Address::Virtual,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_codec::BitField;
    use ferrowl_test_support::reserve_temp_dir;
    use ferrowl_util::convert::{Converter, FileType};

    fn sample() -> MonitorDeviceConfig {
        let definitions = vec![MonitorRegisterDef {
            name: "power".to_string(),
            slave_id: 3,
            kind: Kind::HoldingRegister,
            address: Some(10),
            is_virtual: false,
            value_type: ValueType::U16,
            endian: EndianCfg::default(),
            word_order: WordOrderCfg::default(),
            resolution: 0.1,
            bitmask: None,
            length: 1,
            alignment: AlignmentCfg::default(),
            values: vec![],
            description: "Active power".to_string(),
            default: None,
        }];
        MonitorDeviceConfig {
            version: Some("0.1.0".to_string()),
            reconnect: Some(false),
            log_file: None,
            definitions,
        }
    }

    /// api-contract.md §6 — a monitor device config round-trips through TOML and JSON with no
    /// field loss.
    #[test]
    fn ut_monitor_device_config_roundtrip() {
        let original = sample();
        let dir = reserve_temp_dir("ferrowl_modbus_monitor_device");
        for (ty, ext) in [(FileType::Toml, "toml"), (FileType::Json, "json")] {
            let path = dir.join(format!("monitor-device.{ext}"));
            let path = path.to_str().unwrap();
            Converter::save(&original, path, ty).expect("save");
            let back: MonitorDeviceConfig = Converter::load(path, ty).expect("load");
            assert_eq!(original, back);
        }
    }

    fn write_toml(name: &str, contents: &str) -> (String, ferrowl_test_support::TempDirGuard) {
        let dir = reserve_temp_dir("ferrowl_modbus_monitor_device");
        let path = dir.join(name).to_string_lossy().into_owned();
        std::fs::write(&path, contents).unwrap();
        (path, dir)
    }

    /// api-contract.md §6 — the role-conditional shape ignores (rather than rejects)
    /// client/server-only fields left over in a hand-edited or copy-pasted config file.
    #[test]
    fn ut_monitor_device_config_ignores_unknown_fields() {
        let (path, _dir) = write_toml(
            "ferrowl_monitor_device_unknown_fields.toml",
            r#"
version = "0.1.0"
timeout_ms = 3000
delay_ms = 1000
interval_ms = 1000
script_interval = 1.0
scripts = []

[read_ranges]
holding = "0-10"

definitions = []
"#,
        );
        let cfg: MonitorDeviceConfig =
            Converter::load(&path, FileType::Toml).expect("unknown fields are ignored");
        assert_eq!(cfg.version, Some("0.1.0".to_string()));
        assert!(cfg.definitions.is_empty());
    }

    /// MB-R-145 — `MonitorRegisterDef` has no `access`/`update` field: a hand-written TOML
    /// fragment carrying them deserializes by ignoring the unknown keys, and `.format()`/
    /// `.address()` (the only two `RegisterDef` methods a monitor interpretation needs) work
    /// the same as `RegisterDef`'s.
    #[test]
    fn ut_monitor_register_def_has_no_access_or_update_field() {
        let (path, _dir) = write_toml(
            "ferrowl_monitor_register_def_unknown_fields.toml",
            r#"
name = "power"
slave_id = 1
kind = "HoldingRegister"
address = 5
type = "U16"
access = "readonly"
update = "C_Time:Sleep(1)"
"#,
        );
        let def: MonitorRegisterDef =
            Converter::load(&path, FileType::Toml).expect("unknown fields are ignored");
        assert_eq!(def.address(), Address::Fixed(5));
        assert_eq!(
            def.format(),
            Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default()
            )
        );
    }
}
