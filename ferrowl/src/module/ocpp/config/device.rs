//! OCPP device-type config: the per-device-type settings for an OCPP charging station — the
//! OCPP version it speaks, its role (charging station / management system), the reply timeout,
//! and its Lua simulation scripts. One file = one device type (no ip/port — those are the
//! per-instance endpoint, set via the setup dialog / session like Modbus).
//!
//! The OCPP version lives here (not in the session) because the Lua scripts call version-specific
//! `C_OCPP:<Action>` methods, so a device file is version-locked.

use serde::{Deserialize, Serialize};

pub use crate::config::script::ScriptDef;

use super::session::{OcppRole, OcppSpec, OcppVersion};

/// Optional websocket transport security for an OCPP instance: HTTP Basic Auth (Security Profile
/// one) and TLS/mTLS (Security Profiles two and three). Fields are named by which role uses them;
/// a field irrelevant to the instance's [`OcppRole`] is simply left `None` (same convention as
/// the role-specific fields elsewhere in [`OcppDeviceConfig`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OcppSecurityConfig {
    /// Basic Auth username. Client role: sent on connect. Server role: required to accept a
    /// connection (together with `password`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Basic Auth password. Never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Client role only: extra trust anchor (PEM file) for a self-signed CSMS certificate, added
    /// on top of the system/webpki root store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
    /// Server role only: certificate chain (PEM file) presented to connecting clients. Setting
    /// this (together with `key_file`) is what turns on TLS for the listener.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_file: Option<String>,
    /// Server role only: private key (PEM file) matching `cert_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
    /// Client role: client certificate (PEM file) presented for mutual TLS. Server role: ignored
    /// (see `client_ca_file` for verifying the peer's certificate instead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_file: Option<String>,
    /// Client role only: private key (PEM file) matching `client_cert_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_file: Option<String>,
    /// Server role only: CA (PEM file) used to verify client certificates when
    /// `require_client_cert` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ca_file: Option<String>,
    /// Server role only: reject clients that fail to present a certificate signed by
    /// `client_ca_file` (Security Profile 3).
    #[serde(default)]
    pub require_client_cert: bool,
}

impl OcppSecurityConfig {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Basic Auth credentials, if both `username` and `password` are set.
    pub fn basic_auth(&self) -> Option<ferrowl_ocpp::BasicAuth> {
        match (&self.username, &self.password) {
            (Some(username), Some(password)) => Some(ferrowl_ocpp::BasicAuth {
                username: username.clone(),
                password: password.clone(),
            }),
            _ => None,
        }
    }

    /// CS-side TLS config, if any of `ca_file`/`client_cert_file`/`client_key_file` is set.
    pub fn cs_tls(&self) -> Option<ferrowl_ocpp::CsTlsConfig> {
        if self.ca_file.is_none()
            && self.client_cert_file.is_none()
            && self.client_key_file.is_none()
        {
            return None;
        }
        Some(ferrowl_ocpp::CsTlsConfig {
            ca_file: self.ca_file.clone(),
            client_cert_file: self.client_cert_file.clone(),
            client_key_file: self.client_key_file.clone(),
        })
    }

    /// CSMS-side TLS config, if `cert_file`/`key_file` are set (both required to enable TLS on
    /// the listener).
    pub fn csms_tls(&self) -> Option<ferrowl_ocpp::CsmsTlsConfig> {
        match (&self.cert_file, &self.key_file) {
            (Some(cert_file), Some(key_file)) => Some(ferrowl_ocpp::CsmsTlsConfig {
                cert_file: cert_file.clone(),
                key_file: key_file.clone(),
                client_ca_file: self.client_ca_file.clone(),
                require_client_cert: self.require_client_cert,
            }),
            _ => None,
        }
    }
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

/// A persisted per-connector CSMS RFID accept-list (server role). The connector is identified the
/// same way as [`ConnectorRef`] (`evse` is `None` for 1.6, `Some` for 2.0.1); `rfids` are the tags
/// accepted for that connector *in addition to* the inherited charge-point-wide list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorRfids {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evse: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector: Option<i64>,
    pub rfids: Vec<String>,
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
    /// Charge-point-wide CSMS RFID accept-list (server role): id tags accepted for Authorize /
    /// transaction starts, inherited by every connector. Empty (together with all connector lists)
    /// = accept every tag (the default-accept behaviour). Ignored for the client role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rfids: Vec<String>,
    /// Per-connector CSMS RFID accept-lists (server role), each unioned with [`rfids`](Self::rfids)
    /// when gating that connector's transaction starts. Ignored for the client role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connector_rfids: Vec<ConnectorRfids>,
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
    /// Websocket transport security: Basic Auth and/or TLS/mTLS. Default (all `None`/`false`) is
    /// the pre-existing plain `ws://` behaviour.
    #[serde(default, skip_serializing_if = "OcppSecurityConfig::is_empty")]
    pub security: OcppSecurityConfig,
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
            connector_rfids: Vec::new(),
            connectors: Vec::new(),
            config: Vec::new(),
            security: OcppSecurityConfig::default(),
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
            connector_rfids: vec![ConnectorRfids {
                evse: Some(1),
                connector: Some(2),
                rfids: vec!["CONN2TAG".into()],
            }],
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
            security: OcppSecurityConfig {
                username: Some("cp001".into()),
                password: Some("s3cret".into()),
                ca_file: Some("/tmp/ca.pem".into()),
                cert_file: None,
                key_file: None,
                client_cert_file: None,
                client_key_file: None,
                client_ca_file: None,
                require_client_cert: false,
            },
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
    fn ut_device_config_without_security_section_still_parses() {
        // Pre-existing config files (written before Security Profiles were added) have no
        // `security` table/key at all; `#[serde(default)]` must fill it in as the all-`None`
        // default rather than failing to parse.
        let json = serde_json::json!({
            "ocpp_version": "1.6",
            "role": "client",
            "timeout_ms": 5000,
        });
        let cfg: OcppDeviceConfig = serde_json::from_value(json).expect("old-style config parses");
        assert_eq!(cfg.security, OcppSecurityConfig::default());
        assert!(cfg.security.basic_auth().is_none());
        assert!(cfg.security.cs_tls().is_none());
        assert!(cfg.security.csms_tls().is_none());
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
            security: OcppSecurityConfig::default(),
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
