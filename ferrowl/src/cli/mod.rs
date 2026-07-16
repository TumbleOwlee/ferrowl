//! Command-line interface. Modules can be supplied ad-hoc with repeatable
//! `--module key=val,...` flags and/or pre-configured `--session <file>` files; both resolve
//! to the same [`ModuleSpec`] list.

use std::collections::HashMap;

use clap::{Args, Parser, Subcommand};

pub mod headless;

use crate::config::ocpp::OcppProtocol;
use crate::config::{self, Endpoint, ModuleSpec, OcppModuleSpec, Role};

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

    /// Run configured modules without the TUI (headless/CI mode). See [`crate::cli::headless::run`]
    /// for the exit-code contract.
    Run(RunArgs),
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

#[derive(Args, Debug)]
pub struct RunArgs {
    /// A session file listing multiple module instances (repeatable).
    #[arg(long = "session", value_name = "FILE")]
    pub sessions: Vec<String>,

    /// A module to start, e.g.
    /// --module name=evse-1,device=configs/evse.toml,transport=tcp,ip=10.0.0.5,port=502,role=server
    #[arg(long = "module", value_name = "KEY=VAL,...")]
    pub modules: Vec<String>,

    /// An ad-hoc OCPP module to start, e.g.
    /// --ocpp name=cs-1,device=configs/cs.toml,protocol=ws,ip=127.0.0.1,port=9000,path=/ocpp/cp001
    #[arg(long = "ocpp", value_name = "KEY=VAL,...")]
    pub ocpp: Vec<String>,

    /// Run for this many seconds then exit cleanly (code 0). Omit to run until Ctrl-C.
    #[arg(long, value_name = "SECS")]
    pub duration: Option<u64>,

    /// Append every drained log line to this file too (in addition to stdout).
    #[arg(long = "log-file", value_name = "FILE")]
    pub log_file: Option<String>,

    /// Exit with code 2 (after stopping every module) if a drained log line starts with the
    /// `[sim]` prefix Lua script errors are logged under. This is plain log-string detection, not
    /// a structured error channel, so it only catches errors that are actually logged.
    #[arg(long = "exit-on-error")]
    pub exit_on_error: bool,
}

impl RunArgs {
    /// Resolve modbus module specs the same way the TUI path does: delegate to
    /// [`CliArgs::module_specs`] over the equivalent `--session`/`--module` flags.
    pub fn module_specs(&self) -> Result<Vec<ModuleSpec>, String> {
        self.as_cli_args().module_specs()
    }

    /// Resolve OCPP module specs from `--session` files (same as the TUI path via
    /// [`CliArgs::ocpp_specs`]), plus any ad-hoc `--ocpp key=val,...` flags.
    pub fn ocpp_specs(&self) -> Result<Vec<OcppModuleSpec>, String> {
        let mut specs = self.as_cli_args().ocpp_specs()?;
        for spec in &self.ocpp {
            specs.push(parse_ocpp_spec(spec)?);
        }
        Ok(specs)
    }

    fn as_cli_args(&self) -> CliArgs {
        CliArgs {
            command: None,
            modules: self.modules.clone(),
            sessions: self.sessions.clone(),
            devices: Vec::new(),
            demo: false,
        }
    }
}

impl CliArgs {
    /// Resolve every module instance from `--session` files (first) and `--module` flags.
    /// Session modules are stored as `serde_json::Value`; we dispatch on the `"type"` field
    /// (defaulting to `"modbus"`) to deserialize the right spec type.
    pub fn module_specs(&self) -> Result<Vec<ModuleSpec>, String> {
        let mut specs = Vec::new();
        for path in &self.sessions {
            let session = config::load_session(path).map_err(|e| e.to_string())?;
            for module_val in session.modules {
                let ty = module_val
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("modbus");
                match ty {
                    "modbus" => {
                        let spec: ModuleSpec = serde_json::from_value(module_val)
                            .map_err(|e| format!("invalid modbus module spec: {e}"))?;
                        specs.push(spec);
                    }
                    // OCPP modules are resolved separately by `ocpp_specs`.
                    "ocpp" => {}
                    other => {
                        return Err(format!("unsupported module type '{other}'"));
                    }
                }
            }
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

    /// Resolve every OCPP module instance from `--session` files (modules tagged
    /// `"type":"ocpp"`). Each entry carries the device-config path + endpoint; the device file
    /// (version/role/timeout/scripts) is loaded separately when the tab is built.
    pub fn ocpp_specs(&self) -> Result<Vec<OcppModuleSpec>, String> {
        let mut specs = Vec::new();
        for path in &self.sessions {
            let session = config::load_session(path).map_err(|e| e.to_string())?;
            for module_val in session.modules {
                let ty = module_val.get("type").and_then(|v| v.as_str());
                if ty == Some("ocpp") {
                    let spec: OcppModuleSpec = serde_json::from_value(module_val)
                        .map_err(|e| format!("invalid ocpp module spec: {e}"))?;
                    specs.push(spec);
                }
            }
        }
        Ok(specs)
    }
}

/// Build a [`ModuleSpec`] for a `--device` flag: a TCP client polling the default demo
/// endpoint 127.0.0.1:5020 (matches the `--demo` server). Endpoint/role are not
/// configurable here — use `--module` for full control.
pub fn create_module_spec_by_device(name: String, device: String) -> ModuleSpec {
    ModuleSpec {
        name,
        device,
        role: Role::Client,
        endpoint: Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 5020,
        },
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
    })
}

/// Parse a single `--ocpp` value (`key=val,key=val,...`) into an [`OcppModuleSpec`]. Mirrors
/// [`parse_module_spec`]; role/version/timeout/scripts live in the referenced device file, not
/// on the command line.
pub fn parse_ocpp_spec(input: &str) -> Result<OcppModuleSpec, String> {
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

    let name = get("name").ok_or("ocpp module requires 'name'")?;
    let device = get("device").ok_or("ocpp module requires 'device'")?;
    let ip = get("ip").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = get("port")
        .ok_or("ocpp module requires 'port'")?
        .parse()
        .map_err(|_| "invalid 'port'")?;
    let path = get("path").unwrap_or_default();
    let protocol = match get("protocol").as_deref() {
        None | Some("ws") => OcppProtocol::Ws,
        Some("wss") => OcppProtocol::Wss,
        Some(other) => return Err(format!("invalid protocol '{other}' (expected ws|wss)")),
    };

    Ok(OcppModuleSpec {
        name,
        device,
        protocol,
        ip,
        port,
        path,
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
    /// CL-R-002 — a --module TCP descriptor parses into a module instance.
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
    /// CL-R-002 — a --module TCP descriptor applies its documented defaults.
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
    /// CL-R-002 — a --module RTU descriptor parses into a module instance.
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
    /// CL-R-004 — a --device occurrence auto-builds a TCP client with the fixed endpoint and default name.
    fn ut_device_spec_defaults() {
        let spec = create_module_spec_by_device("Device 0".to_string(), "d.toml".to_string());
        assert_eq!(spec.name, "Device 0");
        assert_eq!(spec.device, "d.toml");
        assert_eq!(spec.role, Role::Client);
        assert_eq!(
            spec.endpoint,
            Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020
            }
        );
    }

    #[test]
    /// CL-R-002 — a --module descriptor missing its name is a parse error.
    fn ut_missing_name_errors() {
        assert!(parse_module_spec("device=d.toml,port=502").is_err());
    }

    #[test]
    /// CL-R-002 — a --module descriptor missing its port is a parse error.
    fn ut_missing_port_errors() {
        assert!(parse_module_spec("name=m,device=d.toml,transport=tcp").is_err());
    }

    #[test]
    /// CL-R-007 — the instance set concatenates session, --module, then --device sources in order.
    fn ut_module_specs_combines_all_sources() {
        use ferrowl_util::convert::{Converter, FileType};
        let session = config::Session {
            version: None,
            modules: vec![
                serde_json::to_value(create_module_spec_by_device("S".into(), "s.toml".into()))
                    .unwrap(),
            ],
            scripts: vec![],
            interval: 1.0,
        };
        let path = std::env::temp_dir().join("ferrowl_cli_session.toml");
        let path = path.to_str().unwrap().to_string();
        Converter::save(&session, &path, FileType::Toml).unwrap();

        let args = CliArgs {
            command: None,
            modules: vec!["name=m,device=d.toml,port=1".into()],
            sessions: vec![path],
            devices: vec!["dev.toml".into()],
            demo: false,
        };
        let specs = args.module_specs().unwrap();
        assert_eq!(specs.len(), 3); // session + module + device
    }

    #[test]
    /// CL-R-003 — a --session file's instances resolve, split into modbus and ocpp.
    fn ut_session_splits_modbus_and_ocpp() {
        use ferrowl_util::convert::{Converter, FileType};
        let mut modbus =
            serde_json::to_value(create_module_spec_by_device("mb".into(), "s.toml".into()))
                .unwrap();
        modbus
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());
        let mut ocpp = serde_json::to_value(OcppModuleSpec {
            name: "cs".into(),
            device: "cs.toml".into(),
            protocol: config::ocpp::OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
        })
        .unwrap();
        ocpp.as_object_mut()
            .unwrap()
            .insert("type".into(), "ocpp".into());

        let session = config::Session {
            version: None,
            modules: vec![modbus, ocpp],
            scripts: vec![],
            interval: 1.0,
        };
        let path = std::env::temp_dir().join("ferrowl_cli_mixed_session.json");
        let path = path.to_str().unwrap().to_string();
        Converter::save(&session, &path, FileType::Json).unwrap();

        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec![path],
            devices: vec![],
            demo: false,
        };
        // Modbus loader sees only the modbus module; OCPP loader sees only the ocpp module.
        assert_eq!(args.module_specs().unwrap().len(), 1);
        let ocpp = args.ocpp_specs().unwrap();
        assert_eq!(ocpp.len(), 1);
        assert_eq!(ocpp[0].name, "cs");
        assert_eq!(ocpp[0].device, "cs.toml");
        assert_eq!(ocpp[0].port, 9000);
    }

    #[test]
    /// CL-R-003 — a --session file that fails to load surfaces an error during resolution.
    fn ut_module_specs_session_load_error() {
        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec!["/no/such/ferrowl.toml".into()],
            devices: vec![],
            demo: false,
        };
        assert!(args.module_specs().is_err());
    }

    #[test]
    /// CL-R-002 — the descriptor mini-language rejects empty parts and malformed values.
    fn ut_parse_empty_parts_and_error_paths() {
        // Empty comma segment is skipped.
        assert_eq!(
            parse_module_spec("name=m,,device=d.toml,port=1")
                .unwrap()
                .name,
            "m"
        );
        // Invalid role / transport.
        assert!(parse_module_spec("name=m,device=d,port=1,role=bogus").is_err());
        assert!(parse_module_spec("name=m,device=d,transport=usb").is_err());
        // Segment without '='.
        assert!(parse_module_spec("name=m,oops,device=d,port=1").is_err());
        // RTU missing path; invalid numeric option; invalid port.
        assert!(parse_module_spec("name=m,device=d,transport=rtu").is_err());
        assert!(parse_module_spec("name=m,device=d,transport=rtu,path=/x,data_bits=foo").is_err());
        assert!(parse_module_spec("name=m,device=d,port=notanum").is_err());
    }

    #[test]
    /// CL-R-002 — a --module RTU descriptor parses its full option set.
    fn ut_parse_rtu_full_options() {
        let spec = parse_module_spec(
            "name=m,device=d,transport=rtu,path=/dev/x,baud_rate=4800,parity=even,data_bits=7,stop_bits=2",
        )
        .unwrap();
        assert_eq!(
            spec.endpoint,
            Endpoint::Rtu {
                path: "/dev/x".into(),
                baud_rate: 4800,
                parity: Some("even".into()),
                data_bits: Some(7),
                stop_bits: Some(2),
            }
        );
    }

    #[test]
    /// CL-R-013 — a run --ocpp descriptor applies its documented defaults.
    fn ut_parse_ocpp_spec_defaults() {
        let spec = parse_ocpp_spec("name=cs-1,device=cs.toml,port=9000").unwrap();
        assert_eq!(spec.name, "cs-1");
        assert_eq!(spec.device, "cs.toml");
        assert_eq!(spec.ip, "127.0.0.1");
        assert_eq!(spec.port, 9000);
        assert_eq!(spec.path, "");
        assert_eq!(spec.protocol, config::ocpp::OcppProtocol::Ws);
    }

    #[test]
    /// CL-R-013 — a run --ocpp descriptor parses its full option set.
    fn ut_parse_ocpp_spec_full() {
        let spec = parse_ocpp_spec(
            "name=cs-1,device=cs.toml,protocol=wss,ip=10.0.0.5,port=9001,path=/ocpp/cp001",
        )
        .unwrap();
        assert_eq!(spec.protocol, config::ocpp::OcppProtocol::Wss);
        assert_eq!(spec.ip, "10.0.0.5");
        assert_eq!(spec.path, "/ocpp/cp001");
    }

    #[test]
    /// CL-R-013 — a malformed run --ocpp descriptor is a parse error.
    fn ut_parse_ocpp_spec_errors() {
        assert!(parse_ocpp_spec("device=d,port=1").is_err()); // missing name
        assert!(parse_ocpp_spec("name=m,port=1").is_err()); // missing device
        assert!(parse_ocpp_spec("name=m,device=d").is_err()); // missing port
        assert!(parse_ocpp_spec("name=m,device=d,port=notanum").is_err());
        assert!(parse_ocpp_spec("name=m,device=d,port=1,protocol=bogus").is_err());
    }

    #[test]
    /// CL-R-013 — the run subcommand parses its own flag set.
    fn ut_run_subcommand_parses() {
        let args = CliArgs::parse_from([
            "ferrowl",
            "run",
            "--module",
            "name=m,device=d.toml,port=1502",
            "--ocpp",
            "name=cs,device=cs.toml,port=9000",
            "--duration",
            "5",
            "--log-file",
            "out.log",
            "--exit-on-error",
        ]);
        match args.command {
            Some(SubCommand::Run(run)) => {
                assert_eq!(
                    run.modules,
                    vec!["name=m,device=d.toml,port=1502".to_string()]
                );
                assert_eq!(
                    run.ocpp,
                    vec!["name=cs,device=cs.toml,port=9000".to_string()]
                );
                assert_eq!(run.duration, Some(5));
                assert_eq!(run.log_file.as_deref(), Some("out.log"));
                assert!(run.exit_on_error);
            }
            _ => panic!("expected SubCommand::Run"),
        }
    }

    #[test]
    /// CL-R-013 — run resolves modbus from --session/--module and ocpp from --session/--ocpp.
    fn ut_run_args_resolve_module_and_ocpp_specs() {
        let run = RunArgs {
            sessions: vec![],
            modules: vec!["name=m,device=d.toml,port=1502".into()],
            ocpp: vec!["name=cs,device=cs.toml,port=9000".into()],
            duration: None,
            log_file: None,
            exit_on_error: false,
        };
        let specs = run.module_specs().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "m");
        let ocpp = run.ocpp_specs().unwrap();
        assert_eq!(ocpp.len(), 1);
        assert_eq!(ocpp[0].name, "cs");
        assert_eq!(ocpp[0].port, 9000);
    }
}
