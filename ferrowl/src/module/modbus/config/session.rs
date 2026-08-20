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
    /// The sim cycle interval as a `Duration`; see [`crate::config::sanitize_interval_secs`] for
    /// the sanitization rule (no floor here — the session dialog has always allowed arbitrarily
    /// small positive intervals).
    pub fn interval_duration(&self) -> std::time::Duration {
        crate::config::sanitize_interval_secs(self.interval, default_interval(), 0.0)
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

/// Whether a module polls a remote device (client), simulates one (server), or passively
/// observes bus traffic between other devices (monitor, MB-R-076/MB-R-140–145).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Client,
    #[default]
    Server,
    Monitor,
}

impl std::fmt::Display for Role {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Client => write!(fmt, "Client"),
            Role::Server => write!(fmt, "Server"),
            Role::Monitor => write!(fmt, "Monitor"),
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
    /// RTU framing carried over a TCP socket (MB-R-113); `rename_all = "lowercase"` alone
    /// would tag this `"rtuovertcp"`, not the required `"rtu_over_tcp"`.
    #[serde(rename = "rtu_over_tcp")]
    RtuOverTcp {
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
    /// MB-R-116 — same field set as `Tcp`; its own variant since it carries a distinct
    /// `ferrowl_modbus::udp::Config` (no `tls`) once resolved.
    Udp {
        ip: String,
        port: u16,
    },
    /// ASCII framing carried over a TCP socket (MB-R-125); same field set as `Tcp`/
    /// `RtuOverTcp`, no separate struct. `rename_all = "lowercase"` alone would tag this
    /// `"asciiovertcp"`, not the required `"ascii_over_tcp"`.
    #[serde(rename = "ascii_over_tcp")]
    AsciiOverTcp {
        ip: String,
        port: u16,
    },
    /// ASCII framing over a serial line (MB-R-121); same field set as `Rtu`, no separate
    /// struct. `rename_all = "lowercase"` alone already produces the correct tag `"ascii"`
    /// here (unlike the compound-name variants above), so no `#[serde(rename)]` needed.
    Ascii {
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
            Endpoint::RtuOverTcp { ip, port } => {
                write!(fmt, "{}:{} (rtu/tcp)", ip, port)
            }
            Endpoint::Udp { ip, port } => {
                write!(fmt, "{}:{} (udp)", ip, port)
            }
            Endpoint::AsciiOverTcp { ip, port } => {
                write!(fmt, "{}:{} (ascii/tcp)", ip, port)
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
            Endpoint::Ascii {
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
                    "{},{},{},{},{} (ascii)",
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
    /// CS-R-033 — a session file round-trips its module instances, scripts, and interval.
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
    /// CS-R-012 — a module entry with no `type` tag loads as modbus.
    fn ut_session_old_format_compat() {
        let json = r#"{"modules":[]}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert!(session.scripts.is_empty());
        assert_eq!(session.interval, 1.0);
    }

    // A hand-edited `interval` that is NaN, negative, or zero must fall back to the 1.0s
    // default instead of panicking or busy-waiting; a valid value converts as-is.
    #[test]
    /// CS-R-017 — a non-finite/non-positive session interval falls back to 1.0s (no floor).
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

    /// MB-R-076 — `Role` gains a `monitor` variant: round-trips through serde as `"monitor"`,
    /// `Display` says `"Monitor"`.
    #[test]
    fn ut_role_serde_monitor_tag_and_display() {
        assert_eq!(Role::Monitor.to_string(), "Monitor");
        assert_eq!(
            serde_json::to_value(Role::Monitor).unwrap(),
            serde_json::json!("monitor")
        );
        assert_eq!(
            serde_json::from_value::<Role>(serde_json::json!("monitor")).unwrap(),
            Role::Monitor
        );
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
        assert_eq!(
            Endpoint::RtuOverTcp {
                ip: "127.0.0.1".into(),
                port: 502
            }
            .to_string(),
            "127.0.0.1:502 (rtu/tcp)"
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
        assert_eq!(
            Endpoint::AsciiOverTcp {
                ip: "127.0.0.1".into(),
                port: 502
            }
            .to_string(),
            "127.0.0.1:502 (ascii/tcp)"
        );
        assert_eq!(
            Endpoint::Ascii {
                path: "/dev/ttyUSB0".into(),
                baud_rate: 9600,
                parity: Some("even".into()),
                data_bits: Some(8),
                stop_bits: Some(1),
            }
            .to_string(),
            "/dev/ttyUSB0,9600,even,8,1 (ascii)"
        );
    }

    #[test]
    fn ut_default_baud() {
        assert_eq!(default_baud(), 19200);
    }

    #[test]
    /// MB-R-113 — the `RtuOverTcp` endpoint variant tags as `rtu_over_tcp` on the
    /// wire (not `rtuovertcp`, which `rename_all = "lowercase"` alone would produce),
    /// and carries exactly `ip`/`port`, the same fields as `Tcp`.
    fn ut_endpoint_rtu_over_tcp_serde_tag() {
        let ep = Endpoint::RtuOverTcp {
            ip: "10.0.0.1".into(),
            port: 502,
        };
        let v = serde_json::to_value(&ep).unwrap();
        assert_eq!(v["transport"], "rtu_over_tcp");
        assert_eq!(v["ip"], "10.0.0.1");
        assert_eq!(v["port"], 502);
        let back: Endpoint = serde_json::from_value(v).unwrap();
        assert_eq!(back, ep);
    }

    #[test]
    /// MB-R-116 — the `Udp` endpoint variant tags as `udp` (`rename_all = "lowercase"`
    /// needs no override here, unlike `rtu_over_tcp`) and carries exactly `ip`/`port`.
    fn ut_endpoint_udp_serde_tag() {
        let ep = Endpoint::Udp {
            ip: "10.0.0.1".into(),
            port: 502,
        };
        let v = serde_json::to_value(&ep).unwrap();
        assert_eq!(v["transport"], "udp");
        assert_eq!(v["ip"], "10.0.0.1");
        assert_eq!(v["port"], 502);
        let back: Endpoint = serde_json::from_value(v).unwrap();
        assert_eq!(back, ep);
    }

    #[test]
    /// MB-R-116 — `Endpoint::Udp` displays as `ip:port (udp)`.
    fn ut_endpoint_udp_display() {
        assert_eq!(
            Endpoint::Udp {
                ip: "127.0.0.1".into(),
                port: 502
            }
            .to_string(),
            "127.0.0.1:502 (udp)"
        );
    }

    #[test]
    /// MB-R-125 — the `AsciiOverTcp` endpoint variant tags as `ascii_over_tcp` on the wire
    /// (`rename_all = "lowercase"` alone would produce `asciiovertcp`) and carries exactly
    /// `ip`/`port`, the same fields as `Tcp`/`RtuOverTcp`.
    fn ut_endpoint_ascii_over_tcp_serde_tag() {
        let ep = Endpoint::AsciiOverTcp {
            ip: "10.0.0.1".into(),
            port: 502,
        };
        let v = serde_json::to_value(&ep).unwrap();
        assert_eq!(v["transport"], "ascii_over_tcp");
        assert_eq!(v["ip"], "10.0.0.1");
        assert_eq!(v["port"], 502);
        let back: Endpoint = serde_json::from_value(v).unwrap();
        assert_eq!(back, ep);
    }

    #[test]
    /// MB-R-121 — the `Ascii` endpoint variant tags as `ascii` and carries exactly the same
    /// fields as `Rtu` (path/baud_rate/parity/data_bits/stop_bits).
    fn ut_endpoint_ascii_serde_tag() {
        let ep = Endpoint::Ascii {
            path: "/dev/ttyUSB0".into(),
            baud_rate: 9600,
            parity: None,
            data_bits: None,
            stop_bits: None,
        };
        let v = serde_json::to_value(&ep).unwrap();
        assert_eq!(v["transport"], "ascii");
        assert_eq!(v["path"], "/dev/ttyUSB0");
        let back: Endpoint = serde_json::from_value(v).unwrap();
        assert_eq!(back, ep);
    }
}
