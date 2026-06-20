//! OCPP device-type config: the per-device-type settings for an OCPP charging station — the
//! OCPP version it speaks, its role (charging station / management system), the reply timeout,
//! and its Lua simulation scripts. One file = one device type (no ip/port — those are the
//! per-instance endpoint, set via the setup dialog / session like Modbus).
//!
//! The OCPP version lives here (not in the session) because the Lua scripts call version-specific
//! `C_OCPP:<Action>` methods, so a device file is version-locked.

use serde::{Deserialize, Serialize};

use super::session::{OcppRole, OcppSpec, OcppVersion};

/// Default for `ScriptDef::enabled`: a script with no explicit flag is active.
fn default_true() -> bool {
    true
}

/// One Lua simulation script attached to an OCPP device type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptDef {
    pub name: String,
    #[serde(default)]
    pub code: String,
    /// Whether the script runs in the simulation loop. Defaults to On (a freshly-created script
    /// and a flag-less file entry are both active).
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// An OCPP device-type configuration file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OcppDeviceConfig {
    /// Ferrowl version that wrote this file, stamped on save. Enables future compatibility shims.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// OCPP protocol version this device type speaks.
    #[serde(default)]
    pub ocpp_version: OcppVersion,
    /// Whether the module acts as a charging station (client) or management system (server).
    #[serde(default)]
    pub role: OcppRole,
    /// Awaited-reply timeout (ms); `None` uses the crate default (30_000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Lua simulation scripts (run every ~100ms while enabled; client role only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<ScriptDef>,
}

impl OcppDeviceConfig {
    /// Assemble a device config from a runtime spec, carrying the given scripts. Used when a
    /// setup/edit dialog supplies version/role/timeout and the scripts are preserved separately.
    pub fn from_spec(spec: &OcppSpec, scripts: Vec<ScriptDef>) -> Self {
        Self {
            version: None,
            ocpp_version: spec.version,
            role: spec.role,
            timeout_ms: spec.timeout_ms,
            scripts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_util::convert::{Converter, FileType};

    #[test]
    fn ut_script_enabled_defaults_true() {
        // A file entry without an `enabled` flag deserializes as active.
        let s: ScriptDef = serde_json::from_str(r#"{"name":"a","code":"x = 1"}"#).unwrap();
        assert!(s.enabled);
    }

    #[test]
    fn ut_device_config_roundtrip() {
        let cfg = OcppDeviceConfig {
            version: Some("0.1.0".into()),
            ocpp_version: OcppVersion::V2_0_1,
            role: OcppRole::Client,
            timeout_ms: Some(5000),
            scripts: vec![ScriptDef {
                name: "boot".into(),
                code: "C_OCPP:Set(\"Power\", 11000)".into(),
                enabled: false,
            }],
        };
        for (ty, ext) in [(FileType::Toml, "toml"), (FileType::Json, "json")] {
            let path = std::env::temp_dir().join(format!("ferrowl_ocpp_device_test.{ext}"));
            let path = path.to_str().unwrap();
            Converter::save(&cfg, path, ty).expect("save");
            let back: OcppDeviceConfig = Converter::load(path, ty).expect("load");
            assert_eq!(cfg, back);
        }
    }

    #[test]
    fn ut_from_spec_carries_scripts() {
        let spec = OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Server,
            protocol: super::super::session::OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
            timeout_ms: Some(1000),
        };
        let scripts = vec![ScriptDef {
            name: "s".into(),
            code: "".into(),
            enabled: true,
        }];
        let cfg = OcppDeviceConfig::from_spec(&spec, scripts.clone());
        assert_eq!(cfg.ocpp_version, OcppVersion::V1_6);
        assert_eq!(cfg.role, OcppRole::Server);
        assert_eq!(cfg.timeout_ms, Some(1000));
        assert_eq!(cfg.scripts, scripts);
        assert_eq!(cfg.version, None);
    }
}
