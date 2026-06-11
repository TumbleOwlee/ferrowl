//! Command-line interface. Modules can be supplied ad-hoc with repeatable
//! `--module key=val,...` flags and/or pre-configured `--session <file>` files; both resolve
//! to the same [`ModuleSpec`] list.

use std::collections::HashMap;

use clap::{Args, Parser, Subcommand};

use crate::config::{self, Endpoint, ModuleSpec, Role};

#[derive(Parser, Debug)]
#[command(version, about = "Ferrowl — a modbus client/server TUI", long_about = None)]
pub struct CliArgs {
    /// Migrate a v0.3.9 config to the current device config format instead of starting the TUI.
    #[command(subcommand)]
    pub command: Option<SubCommand>,

    /// A module to start, e.g.
    /// --module name=evse-1,device=configs/evse.toml,transport=tcp,ip=10.0.0.5,port=502,role=server
    #[arg(long = "module", value_name = "KEY=VAL,...")]
    pub modules: Vec<String>,

    /// A session file listing multiple module instances (repeatable).
    #[arg(long = "session", value_name = "FILE")]
    pub sessions: Vec<String>,

    /// A device configuration used to initialize module.
    #[arg(long = "device", value_name = "FILE")]
    pub devices: Vec<String>,

    /// Demo mode
    #[arg(long)]
    pub demo: bool,
}

#[derive(Subcommand, Debug)]
pub enum SubCommand {
    /// Migrate a v0.3.9 (`modbus-cli-rs`) configuration file to the current device config format.
    ///
    /// Reads a TOML or JSON config from INPUT and writes a converted DeviceConfig to OUTPUT.
    /// Warnings about dropped or approximated fields are printed to stderr.
    Migrate(MigrateArgs),
}

#[derive(Args, Debug)]
pub struct MigrateArgs {
    /// Path to the v0.3.9 configuration file (.toml or .json).
    #[arg(long, short, value_name = "FILE")]
    pub input: String,

    /// Destination path for the converted device config (.toml or .json).
    #[arg(long, short, value_name = "FILE")]
    pub output: String,
}

impl CliArgs {
    /// Resolve every module instance from `--session` files (first) and `--module` flags.
    pub fn module_specs(&self) -> Result<Vec<ModuleSpec>, String> {
        let mut specs = Vec::new();
        for path in &self.sessions {
            let session = config::load_session(path).map_err(|e| e.to_string())?;
            specs.extend(session.modules);
        }
        for spec in &self.modules {
            specs.push(parse_module_spec(spec)?);
        }
        for (num, device) in self.devices.iter().enumerate() {
            specs.push(create_module_spec_by_device(
                format!("Device {num}"),
                device.clone(),
            ));
        }
        Ok(specs)
    }
}

/// Parse a single `--device` value into a [`ModuleSpec`].
pub fn create_module_spec_by_device(name: String, device: String) -> ModuleSpec {
    ModuleSpec {
        name,
        device,
        role: Role::Client,
        endpoint: Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 5020,
        },
        timeout_ms: None,
        delay_ms: None,
        interval_ms: None,
    }
}

/// Parse a single `--module` value (`key=val,key=val,...`) into a [`ModuleSpec`].
pub fn parse_module_spec(input: &str) -> Result<ModuleSpec, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got '{part}'"))?;
        map.insert(key.trim().to_string(), value.trim().to_string());
    }

    let get = |k: &str| map.get(k).cloned();

    let name = get("name").ok_or("module requires 'name'")?;
    let device = get("device")
        .or_else(|| get("type"))
        .ok_or("module requires 'device' (or 'type')")?;

    let role = match get("role").as_deref() {
        None | Some("server") => Role::Server,
        Some("client") => Role::Client,
        Some(other) => return Err(format!("invalid role '{other}' (expected client|server)")),
    };

    let transport = get("transport").unwrap_or_else(|| "tcp".to_string());
    let endpoint = match transport.as_str() {
        "tcp" => Endpoint::Tcp {
            ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
            port: get("port")
                .ok_or("tcp module requires 'port'")?
                .parse()
                .map_err(|_| "invalid 'port'")?,
        },
        "rtu" => Endpoint::Rtu {
            path: get("path").ok_or("rtu module requires 'path'")?,
            baud_rate: parse_opt(get("baud").or_else(|| get("baud_rate")), "baud")?
                .unwrap_or(19200),
            parity: get("parity"),
            data_bits: parse_opt(get("data_bits"), "data_bits")?,
            stop_bits: parse_opt(get("stop_bits"), "stop_bits")?,
        },
        other => return Err(format!("invalid transport '{other}' (expected tcp|rtu)")),
    };

    Ok(ModuleSpec {
        name,
        device,
        role,
        endpoint,
        timeout_ms: None,
        delay_ms: None,
        interval_ms: None,
    })
}

fn parse_opt<T: std::str::FromStr>(
    value: Option<String>,
    field: &str,
) -> Result<Option<T>, String> {
    value
        .map(|v| v.parse::<T>().map_err(|_| format!("invalid '{field}'")))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_parse_tcp_module() {
        let spec = parse_module_spec(
            "name=evse-1,device=configs/evse.toml,transport=tcp,ip=10.0.0.5,port=502,role=server",
        )
        .unwrap();
        assert_eq!(spec.name, "evse-1");
        assert_eq!(spec.device, "configs/evse.toml");
        assert_eq!(spec.role, Role::Server);
        assert_eq!(
            spec.endpoint,
            Endpoint::Tcp {
                ip: "10.0.0.5".into(),
                port: 502
            }
        );
    }

    #[test]
    fn ut_parse_tcp_defaults() {
        // ip and role default; type is an alias for device.
        let spec = parse_module_spec("name=m,type=d.toml,port=1502").unwrap();
        assert_eq!(spec.role, Role::Server);
        assert_eq!(
            spec.endpoint,
            Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 1502
            }
        );
    }

    #[test]
    fn ut_parse_rtu_module() {
        let spec = parse_module_spec(
            "name=m,device=d.toml,transport=rtu,path=/dev/ttyUSB0,baud=9600,role=client",
        )
        .unwrap();
        assert_eq!(spec.role, Role::Client);
        assert_eq!(
            spec.endpoint,
            Endpoint::Rtu {
                path: "/dev/ttyUSB0".into(),
                baud_rate: 9600,
                parity: None,
                data_bits: None,
                stop_bits: None,
            }
        );
    }

    #[test]
    fn ut_missing_name_errors() {
        assert!(parse_module_spec("device=d.toml,port=502").is_err());
    }

    #[test]
    fn ut_missing_port_errors() {
        assert!(parse_module_spec("name=m,device=d.toml,transport=tcp").is_err());
    }
}
