//! Session config: a list of module instances to start (device-type file + per-instance
//! endpoint/role/name). Used by `--session <file>`; a single instance is also built from
//! `--module key=val` on the CLI.

use serde::{Deserialize, Serialize};

/// A pre-configured set of module instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Session {
    /// Ferrowl version that wrote this file, stamped on save. Enables future compatibility
    /// shims when loading configs produced by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub modules: Vec<ModuleSpec>,
}

/// One module instance: which device type, named, with a role and an endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub name: String,
    /// Path to the device-type config file.
    pub device: String,
    #[serde(default)]
    pub role: Role,
    pub endpoint: Endpoint,
    /// Per-instance timing overrides (ms). When unset, the device config — then the built-in
    /// default — supplies the value. See `Module::resolve_timing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<usize>,
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

    fn sample() -> Session {
        Session {
            version: Some("0.1.0".into()),
            modules: vec![
                ModuleSpec {
                    name: "evse-1".into(),
                    device: "configs/evse.toml".into(),
                    role: Role::Server,
                    endpoint: Endpoint::Tcp {
                        ip: "127.0.0.1".into(),
                        port: 5021,
                    },
                    timeout_ms: None,
                    delay_ms: None,
                    interval_ms: None,
                },
                ModuleSpec {
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
                    timeout_ms: Some(750),
                    delay_ms: None,
                    interval_ms: Some(1500),
                },
            ],
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
}
