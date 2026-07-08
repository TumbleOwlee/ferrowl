//! Session config: a list of module instances to start (device-type file + per-instance
//! endpoint/role/name). Used by `--session <file>`; a single instance is also built from
//! `--module key=val` on the CLI.

use crate::config::script::ScriptDef;
use serde::{Deserialize, Serialize};

/// A pre-configured set of module instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Ferrowl version that wrote this file, stamped on save. Enables future compatibility
    /// shims when loading configs produced by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Opaque per-module config blobs. Each entry must include a `"type"` field (e.g.
    /// `"modbus"`) so the loader can dispatch to the right deserializer. Older session
    /// files without a `"type"` field are assumed to be `"modbus"` for compatibility.
    #[serde(default)]
    pub modules: Vec<serde_json::Value>,
    /// Session-level Lua scripts, run in their own Lua state with `C_Module` access to every
    /// module in the session. Older session files without this field load as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<ScriptDef>,
    /// Session sim cycle interval in seconds. Older session files without this field load as
    /// the default (1.0s).
    #[serde(default = "default_interval")]
    pub interval: f64,
}

fn default_interval() -> f64 {
    1.0
}

impl Session {
    /// The sim cycle interval as a `Duration`. A hand-edited file can carry a non-finite or
    /// non-positive `interval`; those fall back to the 1.0s default instead of panicking in
    /// `Duration::from_secs_f64` (NaN/negative) or busy-waiting (zero).
    #[allow(dead_code)] // Wired up by the session-sim UI/headless integration.
    pub fn interval_duration(&self) -> std::time::Duration {
        let secs = if self.interval.is_finite() && self.interval > 0.0 {
            self.interval
        } else {
            default_interval()
        };
        std::time::Duration::from_secs_f64(secs)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: None,
            modules: Vec::new(),
            scripts: Vec::new(),
            interval: default_interval(),
        }
    }
}

/// One module instance: which device type, named, with a role and an endpoint. Timing
/// (timeout/delay/interval ms) is not per-instance — it lives in the device config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub name: String,
    /// Path to the device-type config file.
    pub device: String,
    #[serde(default)]
    pub role: Role,
    pub endpoint: Endpoint,
}

/// Whether a module polls a remote device (client) or simulates one (server).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Client,
    #[default]
    Server,
}

impl std::fmt::Display for Role {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Client => write!(fmt, "Client"),
            Role::Server => write!(fmt, "Server"),
        }
    }
}

/// Transport endpoint for a module instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum Endpoint {
    Tcp {
        ip: String,
        port: u16,
    },
    Rtu {
        path: String,
        #[serde(default = "default_baud")]
        baud_rate: u32,
        #[serde(default)]
        parity: Option<String>,
        #[serde(default)]
        data_bits: Option<u8>,
        #[serde(default)]
        stop_bits: Option<u8>,
    },
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endpoint::Tcp { ip, port } => {
                write!(fmt, "{}:{}", ip, port)
            }
            Endpoint::Rtu {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            } => {
                let data_bits = if let Some(d) = data_bits {
                    format!("{}", d)
                } else {
                    "-".to_string()
                };
                let stop_bits = if let Some(s) = stop_bits {
                    format!("{}", s)
                } else {
                    "-".to_string()
                };
                write!(
                    fmt,
                    "{},{},{},{},{}",
                    path,
                    baud_rate,
                    parity.as_ref().map_or("-", |v| v),
                    data_bits,
                    stop_bits
                )
            }
        }
    }
}

fn default_baud() -> u32 {
    19200
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_util::convert::{Converter, FileType};
    use std::time::Duration;

    fn sample_spec(name: &str, device: &str, role: Role, endpoint: Endpoint) -> serde_json::Value {
        let spec = ModuleSpec {
            name: name.into(),
            device: device.into(),
            role,
            endpoint,
        };
        let mut v = serde_json::to_value(spec).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());
        v
    }

    fn sample() -> Session {
        Session {
            version: Some("0.1.0".into()),
            modules: vec![
                sample_spec(
                    "evse-1",
                    "configs/evse.toml",
                    Role::Server,
                    Endpoint::Tcp {
                        ip: "127.0.0.1".into(),
                        port: 5021,
                    },
                ),
                {
                    let spec = ModuleSpec {
                        name: "meter".into(),
                        device: "configs/meter.toml".into(),
                        role: Role::Client,
                        endpoint: Endpoint::Rtu {
                            path: "/dev/ttyUSB0".into(),
                            baud_rate: 9600,
                            parity: Some("none".into()),
                            data_bits: Some(8),
                            stop_bits: Some(1),
                        },
                    };
                    let mut v = serde_json::to_value(spec).unwrap();
                    v.as_object_mut()
                        .unwrap()
                        .insert("type".into(), "modbus".into());
                    v
                },
            ],
            scripts: vec![ScriptDef {
                name: "s1".into(),
                code: "C_Time:Sleep(1)".into(),
                enabled: true,
            }],
            interval: 2.5,
        }
    }

    #[test]
    fn ut_session_roundtrip() {
        let original = sample();
        for (ty, ext) in [(FileType::Toml, "toml"), (FileType::Json, "json")] {
            let path = std::env::temp_dir().join(format!("ferrowl_session_test.{ext}"));
            let path = path.to_str().unwrap();
            Converter::save(&original, path, ty).expect("save");
            let back: Session = Converter::load(path, ty).expect("load");
            assert_eq!(original, back);
        }
    }

    // An old-format session file (predating `scripts`/`interval`) must still load, with
    // `scripts` defaulting to empty and `interval` to 1.0.
    #[test]
    fn ut_session_old_format_compat() {
        let json = r#"{"modules":[]}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert!(session.scripts.is_empty());
        assert_eq!(session.interval, 1.0);
    }

    // A hand-edited `interval` that is NaN, negative, or zero must fall back to the 1.0s
    // default instead of panicking or busy-waiting; a valid value converts as-is.
    #[test]
    fn ut_session_interval_duration_sanitized() {
        let mut session = Session::default();
        assert_eq!(session.interval_duration(), Duration::from_secs(1));
        session.interval = 0.25;
        assert_eq!(session.interval_duration(), Duration::from_millis(250));
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            session.interval = bad;
            assert_eq!(session.interval_duration(), Duration::from_secs(1));
        }
    }

    #[test]
    fn ut_role_display_and_default() {
        assert_eq!(Role::Client.to_string(), "Client");
        assert_eq!(Role::Server.to_string(), "Server");
        assert_eq!(Role::default(), Role::Server);
    }

    #[test]
    fn ut_endpoint_display() {
        assert_eq!(
            Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 502
            }
            .to_string(),
            "127.0.0.1:502"
        );
        // RTU with all optional fields present.
        assert_eq!(
            Endpoint::Rtu {
                path: "/dev/ttyUSB0".into(),
                baud_rate: 9600,
                parity: Some("even".into()),
                data_bits: Some(8),
                stop_bits: Some(1),
            }
            .to_string(),
            "/dev/ttyUSB0,9600,even,8,1"
        );
        // RTU with the optional fields unset renders dashes.
        assert_eq!(
            Endpoint::Rtu {
                path: "/dev/x".into(),
                baud_rate: 19200,
                parity: None,
                data_bits: None,
                stop_bits: None,
            }
            .to_string(),
            "/dev/x,19200,-,-,-"
        );
    }

    #[test]
    fn ut_default_baud() {
        assert_eq!(default_baud(), 19200);
    }
}
