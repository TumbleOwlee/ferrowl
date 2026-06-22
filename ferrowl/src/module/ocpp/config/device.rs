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

/// A persisted connector entry for a charging-station (client) device type. `evse` is `None` for
/// OCPP 1.6 (connector-only) and `Some` for 2.0.1; `connector` is the connector id. The CS-level
/// entry is implicit (always present in the view) and is not stored here. Maps to a runtime
/// `Scope` when the view is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evse: Option<i64>,
    pub connector: i64,
}

/// A persisted configuration key for a charging-station (client) device type: a name/value pair and
/// its read-only flag, seeded into the client's config store (GetConfiguration / GetVariables) on
/// load and written by `:wd`. Server (CSMS) config is per-connected-station and transient, so it is
/// never persisted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigKeyDef {
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub readonly: bool,
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
    /// Persistent log-file base set via `:log <file>`; `None` disables file logging. The actual
    /// file is `<stem>.<tab-name>.<ext>` next to this path (see `module_log_path`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
    /// CSMS RFID accept-list (server role): id tags accepted for Authorize / StartTransaction.
    /// Empty = accept every tag (the default-accept behaviour). Ignored for the client role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rfids: Vec<String>,
    /// Connector entries for the charging-station (client) view, seeded into its connector table on
    /// load and written by `:wd`. Empty = CS-level only. Ignored for the server role (connectors
    /// there are discovered from connected stations).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<ConnectorRef>,
    /// Persisted configuration keys for the charging-station (client) view, seeded into its config
    /// store on load and written by `:wd`. Empty = use the built-in defaults. Ignored for the server
    /// role (CSMS config is per-connected-station and transient).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<ConfigKeyDef>,
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
            log_file: None,
            rfids: Vec::new(),
            connectors: Vec::new(),
            config: Vec::new(),
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
            log_file: Some("/tmp/ferrowl.log".into()),
            rfids: vec!["DEADBEEF".into(), "CAFE1234".into()],
            connectors: vec![
                ConnectorRef {
                    evse: None,
                    connector: 1,
                },
                ConnectorRef {
                    evse: Some(1),
                    connector: 2,
                },
            ],
            config: vec![
                ConfigKeyDef {
                    key: "HeartbeatInterval".into(),
                    value: "30".into(),
                    readonly: false,
                },
                ConfigKeyDef {
                    key: "NumberOfConnectors".into(),
                    value: "2".into(),
                    readonly: true,
                },
            ],
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
