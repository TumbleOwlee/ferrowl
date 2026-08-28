//! Command-line interface. Modules can be supplied ad-hoc with repeatable
//! `--module key=val,...` flags and/or pre-configured `--session <file>` files; both resolve
//! to the same [`ModuleSpec`] list.

use std::collections::HashMap;

use clap::{Args, Parser, Subcommand};

pub mod bridge;
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

    /// Relay Modbus requests between two interfaces without the TUI: an upstream interface that
    /// bridge mode serves (answering requests, like a real device) and a downstream interface it
    /// connects to as a client, forwarding every request it receives on the upstream side and
    /// relaying the answer back. Useful for placing a TCP-only master in front of a serial-only
    /// device, or vice versa.
    Bridge(BridgeArgs),
}

#[derive(Args, Debug)]
pub struct BridgeArgs {
    /// Required. The interface bridge mode listens on and answers requests from, e.g.
    /// --upstream transport=tcp,ip=0.0.0.0,port=502
    /// or --upstream transport=rtu,path=/dev/ttyUSB0,baud=19200
    #[arg(long, value_name = "KEY=VAL,...")]
    pub upstream: Option<String>,

    /// Required. The interface bridge mode connects to and forwards every upstream request to,
    /// e.g. --downstream transport=tcp,ip=10.0.0.5,port=502
    /// or --downstream transport=rtu,path=/dev/ttyUSB1,baud=19200
    /// Both descriptors accept `transport` (`tcp` default, `rtu`, `rtu_over_tcp`, or
    /// `ascii_over_tcp`), `timeout_ms`, `reconnect` (true/false); `tcp`/`rtu_over_tcp`/
    /// `ascii_over_tcp` also take `ip`, `port`, and TLS keys; `rtu` also takes `path`, `baud`,
    /// `parity`, `data_bits`, `stop_bits`. `--upstream` additionally accepts `unit_ids` (e.g.
    /// `unit_ids=1,3,5-8`) to restrict which slave ids the bridge answers for.
    #[arg(long, value_name = "KEY=VAL,...")]
    pub downstream: Option<String>,

    /// Run for this many seconds then exit cleanly (code 0). Omit to run until Ctrl-C.
    #[arg(long, value_name = "SECS")]
    pub duration: Option<u64>,

    /// Append every drained log line to this file too (in addition to stdout).
    #[arg(long = "log-file", value_name = "FILE")]
    pub log_file: Option<String>,

    /// Exit with code 2 if a drained log line starts with the `[bridge]` prefix bridge errors are
    /// logged under. This is plain log-string detection, not a structured error channel, so it
    /// only catches errors that are actually logged.
    #[arg(long = "exit-on-error")]
    pub exit_on_error: bool,
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

    /// Exit with code 3 (after stopping every module) if a drained log line has log level error.
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
        "rtu_over_tcp" => Endpoint::RtuOverTcp {
            ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
            port: get("port")
                .ok_or("rtu_over_tcp module requires 'port'")?
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
        "udp" => Endpoint::Udp {
            ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
            port: get("port")
                .ok_or("udp module requires 'port'")?
                .parse()
                .map_err(|_| "invalid 'port'")?,
        },
        "ascii" => Endpoint::Ascii {
            path: get("path").ok_or("ascii module requires 'path'")?,
            baud_rate: parse_opt(get("baud").or_else(|| get("baud_rate")), "baud")?
                .unwrap_or(19200),
            parity: get("parity"),
            data_bits: parse_opt(get("data_bits"), "data_bits")?,
            stop_bits: parse_opt(get("stop_bits"), "stop_bits")?,
        },
        "ascii_over_tcp" => Endpoint::AsciiOverTcp {
            ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
            port: get("port")
                .ok_or("ascii_over_tcp module requires 'port'")?
                .parse()
                .map_err(|_| "invalid 'port'")?,
        },
        other => {
            return Err(format!(
                "invalid transport '{other}' (expected tcp|rtu|rtu_over_tcp|udp|ascii|ascii_over_tcp)"
            ));
        }
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

/// System clock before the epoch (misconfigured host) yields 0 rather than killing a running
/// simulation, hence `unwrap_or_default` rather than `expect`.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Open `--log-file` (create-and-append, after `ferrowl_util::path::expand`), or `Ok(None)` when
/// no path was given. On failure, returns the original path alongside the I/O error so the
/// caller can print the exact `Error: failed to open --log-file '{path}': {e}` message and run
/// its own cleanup before choosing its exit code.
pub(crate) fn open_log_file(
    path: Option<&str>,
) -> Result<Option<std::fs::File>, (String, std::io::Error)> {
    match path {
        Some(path) => std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ferrowl_util::path::expand(path))
            .map(Some)
            .map_err(|e| (path.to_string(), e)),
        None => Ok(None),
    }
}

fn parse_opt<T: std::str::FromStr>(
    value: Option<String>,
    field: &str,
) -> Result<Option<T>, String> {
    value
        .map(|v| v.parse::<T>().map_err(|_| format!("invalid '{field}'")))
        .transpose()
}

/// Parse a single `--upstream`/`--downstream` value (`key=val,key=val,...`) into a
/// [`ferrowl_modbus::bridge::BridgeEndpointSpec`] (BR-R-004). Mirrors [`parse_module_spec`].
pub fn parse_bridge_descriptor(
    input: &str,
) -> Result<ferrowl_modbus::bridge::BridgeEndpointSpec, String> {
    // A plain `,`-split would break `unit_ids=1,3,5-8` (BR-R-015's own list/range grammar
    // uses the same comma the descriptor uses between keys): a comma-separated segment with
    // no `=` is a continuation of the previous key's value, not a new key.
    let mut map: HashMap<String, String> = HashMap::new();
    let mut last_key: Option<String> = None;
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('=') {
            Some((key, value)) => {
                let key = key.trim().to_string();
                map.insert(key.clone(), value.trim().to_string());
                last_key = Some(key);
            }
            None => {
                let key = last_key
                    .as_ref()
                    .ok_or_else(|| format!("expected key=value, got '{part}'"))?;
                let entry = map
                    .get_mut(key)
                    .expect("last_key always names a present map entry");
                entry.push(',');
                entry.push_str(part);
            }
        }
    }
    let get = |k: &str| map.get(k).cloned();

    let unit_ids = get("unit_ids")
        .map(|s| ferrowl_modbus::bridge::UnitIdFilter::parse(&s))
        .transpose()?;

    let timeout_ms = parse_opt(get("timeout_ms"), "timeout_ms")?.unwrap_or(3000usize);
    let reconnect = match get("reconnect").as_deref() {
        None => true,
        Some("true") => true,
        Some("false") => false,
        Some(other) => {
            return Err(format!(
                "invalid 'reconnect' value '{other}' (expected true|false)"
            ));
        }
    };

    let transport = get("transport").unwrap_or_else(|| "tcp".to_string());
    let kind = match transport.as_str() {
        "tcp" => {
            let tls = build_descriptor_tls(&get)?;
            ferrowl_modbus::bridge::BridgeEndpointKind::Tcp(ferrowl_modbus::tcp::Config {
                ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
                port: get("port")
                    .ok_or("tcp descriptor requires 'port'")?
                    .parse()
                    .map_err(|_| "invalid 'port'")?,
                timeout_ms,
                delay_ms: 0,
                interval_ms: 0,
                reconnect,
                tls,
            })
        }
        "rtu_over_tcp" => {
            let tls = build_descriptor_tls(&get)?;
            ferrowl_modbus::bridge::BridgeEndpointKind::RtuOverTcp(ferrowl_modbus::tcp::Config {
                ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
                port: get("port")
                    .ok_or("rtu_over_tcp descriptor requires 'port'")?
                    .parse()
                    .map_err(|_| "invalid 'port'")?,
                timeout_ms,
                delay_ms: 0,
                interval_ms: 0,
                reconnect,
                tls,
            })
        }
        "ascii_over_tcp" => {
            let tls = build_descriptor_tls(&get)?;
            ferrowl_modbus::bridge::BridgeEndpointKind::AsciiOverTcp(ferrowl_modbus::tcp::Config {
                ip: get("ip").unwrap_or_else(|| "127.0.0.1".to_string()),
                port: get("port")
                    .ok_or("ascii_over_tcp descriptor requires 'port'")?
                    .parse()
                    .map_err(|_| "invalid 'port'")?,
                timeout_ms,
                delay_ms: 0,
                interval_ms: 0,
                reconnect,
                tls,
            })
        }
        "rtu" => ferrowl_modbus::bridge::BridgeEndpointKind::Rtu(ferrowl_modbus::rtu::Config {
            path: get("path").ok_or("rtu descriptor requires 'path'")?,
            baud_rate: parse_opt(get("baud").or_else(|| get("baud_rate")), "baud")?
                .unwrap_or(19200),
            slave: 1, // BR-R-004/edge-cases.md — inert for bridge, same as an ordinary RTU server.
            parity: get("parity"),
            data_bits: parse_opt(get("data_bits"), "data_bits")?,
            stop_bits: parse_opt(get("stop_bits"), "stop_bits")?,
            timeout_ms,
            delay_ms: 0,
            interval_ms: 0,
            reconnect,
        }),
        other => {
            return Err(format!(
                "invalid transport '{other}' (expected tcp|rtu|rtu_over_tcp|ascii_over_tcp)"
            ));
        }
    };
    Ok(ferrowl_modbus::bridge::BridgeEndpointSpec { kind, unit_ids })
}

/// BR-R-011 — the `tls` field set (MB-R-104–111 field names), tcp-only. This mini-language is
/// flat (`key=val,key=val`), unlike JSON's nested `"tls": {...}` object, so each field is its
/// own top-level descriptor key exactly as spelled in [`ferrowl_modbus::tcp::ModbusTlsConfig`]
/// (no `tls_` prefix — the field names are already unambiguous against the other descriptor
/// keys). "Opt-in" (MB-R-104): `tls` stays `None` unless at least one of these nine keys is
/// present in the descriptor.
fn build_descriptor_tls(
    get: &impl Fn(&str) -> Option<String>,
) -> Result<Option<ferrowl_modbus::tcp::ModbusTlsConfig>, String> {
    let any_present = [
        "ca_file",
        "cert_file",
        "key_file",
        "client_cert_file",
        "client_key_file",
        "client_ca_file",
        "require_client_cert",
        "self_signed",
        "insecure_skip_verify",
    ]
    .iter()
    .any(|k| get(k).is_some());
    if !any_present {
        return Ok(None);
    }
    let parse_bool = |k: &str| -> Result<bool, String> {
        match get(k).as_deref() {
            None => Ok(false),
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            Some(other) => Err(format!(
                "invalid '{k}' value '{other}' (expected true|false)"
            )),
        }
    };
    let server_cert = ferrowl_util::tls::ServerCertSource::resolve(
        parse_bool("self_signed")?,
        get("cert_file"),
        get("key_file"),
    )?;
    // Both `server` and `client` are always present regardless of which role this endpoint
    // turns out to be (MB-R-105) — the descriptor's fields map 1:1 onto whichever half a
    // downstream/upstream role actually consults; the other half is simply inert.
    let server = if parse_bool("require_client_cert")? {
        let ca_files = get("client_ca_file").into_iter().collect::<Vec<_>>();
        ferrowl_util::tls::ServerTlsPolicy::MutualTls {
            server_cert,
            client_verification: ferrowl_util::tls::ClientCertVerification::resolve(
                false, ca_files,
            )?,
        }
    } else {
        ferrowl_util::tls::ServerTlsPolicy::Tls { server_cert }
    };
    let client_verification = ferrowl_util::tls::ClientVerification::resolve(
        parse_bool("insecure_skip_verify")?,
        get("ca_file"),
    );
    let client = match (get("client_cert_file"), get("client_key_file")) {
        (Some(client_cert_file), Some(client_key_file)) => {
            ferrowl_util::tls::ClientTlsPolicy::MutualTls {
                client_verification,
                client_identity: ferrowl_util::tls::ClientCertSource::Explicit {
                    client_cert_file,
                    client_key_file,
                },
            }
        }
        _ => ferrowl_util::tls::ClientTlsPolicy::Tls {
            client_verification,
        },
    };
    Ok(Some(ferrowl_modbus::tcp::ModbusTlsConfig {
        server,
        client,
    }))
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
    /// CL-R-002 — a --module RtuOverTcp descriptor parses like TCP (same ip/port
    /// keys), tagged `rtu_over_tcp`.
    fn ut_parse_rtu_over_tcp_module() {
        let spec = parse_module_spec(
            "name=m,device=d.toml,transport=rtu_over_tcp,ip=10.0.0.5,port=502,role=client",
        )
        .unwrap();
        assert_eq!(spec.role, Role::Client);
        assert_eq!(
            spec.endpoint,
            Endpoint::RtuOverTcp {
                ip: "10.0.0.5".into(),
                port: 502
            }
        );
    }

    #[test]
    /// CL-R-002 — an unknown `transport` value is still a parse error, listing all
    /// six valid options.
    fn ut_parse_invalid_transport_still_errors() {
        assert!(parse_module_spec("name=m,device=d,transport=usb").is_err());
    }

    #[test]
    /// CL-R-002 — a --module Ascii descriptor parses like RTU (same path/baud/parity/
    /// data_bits/stop_bits keys), tagged `ascii`.
    fn ut_parse_ascii_module() {
        let spec = parse_module_spec(
            "name=m,device=d.toml,transport=ascii,path=/dev/ttyUSB0,baud=9600,role=client",
        )
        .unwrap();
        assert_eq!(spec.role, Role::Client);
        assert_eq!(
            spec.endpoint,
            Endpoint::Ascii {
                path: "/dev/ttyUSB0".into(),
                baud_rate: 9600,
                parity: None,
                data_bits: None,
                stop_bits: None
            }
        );
    }

    #[test]
    /// CL-R-002 — a --module AsciiOverTcp descriptor parses like TCP (same ip/port keys),
    /// tagged `ascii_over_tcp`.
    fn ut_parse_ascii_over_tcp_module() {
        let spec = parse_module_spec(
            "name=m,device=d.toml,transport=ascii_over_tcp,ip=10.0.0.5,port=502,role=client",
        )
        .unwrap();
        assert_eq!(spec.role, Role::Client);
        assert_eq!(
            spec.endpoint,
            Endpoint::AsciiOverTcp {
                ip: "10.0.0.5".into(),
                port: 502
            }
        );
    }

    #[test]
    /// MB-R-116 — `transport=udp` parses for either role (no restriction this run).
    fn ut_parse_module_spec_udp_ok() {
        let spec =
            parse_module_spec("name=m,device=d.toml,transport=udp,ip=127.0.0.1,port=502").unwrap();
        assert_eq!(
            spec.endpoint,
            Endpoint::Udp {
                ip: "127.0.0.1".into(),
                port: 502
            }
        );

        let client_spec = parse_module_spec(
            "name=m,device=d.toml,role=client,transport=udp,ip=127.0.0.1,port=502",
        )
        .unwrap();
        assert_eq!(client_spec.role, Role::Client);
        assert!(matches!(client_spec.endpoint, Endpoint::Udp { .. }));
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
    /// OC-R-119 — extra_headers is not a --ocpp key=value field; an unrecognized key is silently
    /// inert, same as config/connectors.
    fn ut_parse_ocpp_spec_ignores_extra_headers_key() {
        let spec = parse_ocpp_spec("name=cs1,device=d.toml,port=9000,extra_headers=X-Tenant:acme")
            .unwrap();
        assert_eq!(spec.name, "cs1");
        assert_eq!(spec.device, "d.toml");
        assert_eq!(spec.port, 9000);
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

    // --- Argument surface: version/help, subcommands, flag scoping, parse errors ----------

    #[test]
    /// CL-R-001 — --version and --help print and exit 0, taking precedence over starting the TUI.
    fn ut_version_and_help_exit_zero() {
        use clap::error::ErrorKind;
        let v = CliArgs::try_parse_from(["ferrowl", "--version"]).unwrap_err();
        assert_eq!(v.kind(), ErrorKind::DisplayVersion);
        assert_eq!(v.exit_code(), 0);
        let h = CliArgs::try_parse_from(["ferrowl", "--help"]).unwrap_err();
        assert_eq!(h.kind(), ErrorKind::DisplayHelp);
        assert_eq!(h.exit_code(), 0);
    }

    #[test]
    /// CL-R-010 — the two subcommands dispatch, replacing the default (start-the-TUI) action.
    fn ut_subcommands_dispatch() {
        assert!(matches!(
            CliArgs::parse_from(["ferrowl", "run"]).command,
            Some(SubCommand::Run(_))
        ));
        assert!(matches!(
            CliArgs::parse_from(["ferrowl", "migrate", "-i", "a.toml", "-o", "b.toml"]).command,
            Some(SubCommand::Migrate(_))
        ));
        // No subcommand → default action (the TUI); `command` stays None.
        assert!(CliArgs::parse_from(["ferrowl"]).command.is_none());
    }

    #[test]
    /// CL-R-011 — the migrate subcommand requires both --input and --output.
    fn ut_migrate_requires_input_and_output() {
        assert!(CliArgs::try_parse_from(["ferrowl", "migrate"]).is_err());
        assert!(CliArgs::try_parse_from(["ferrowl", "migrate", "-i", "a.toml"]).is_err());
        assert!(CliArgs::try_parse_from(["ferrowl", "migrate", "-o", "b.toml"]).is_err());
        match CliArgs::parse_from(["ferrowl", "migrate", "-i", "a.toml", "-o", "b.toml"]).command {
            Some(SubCommand::Migrate(m)) => {
                assert_eq!(m.input, "a.toml");
                assert_eq!(m.output, "b.toml");
            }
            _ => panic!("expected migrate"),
        }
    }

    #[test]
    /// CL-R-014 — --ocpp is accepted only on run; --device only on the top-level command.
    fn ut_ocpp_and_device_flag_scoping() {
        assert!(CliArgs::try_parse_from(["ferrowl", "--ocpp", "name=cs,device=d,port=1"]).is_err());
        assert!(
            CliArgs::try_parse_from(["ferrowl", "run", "--ocpp", "name=cs,device=d,port=1"])
                .is_ok()
        );
        assert!(CliArgs::try_parse_from(["ferrowl", "run", "--device", "d.toml"]).is_err());
        assert!(CliArgs::try_parse_from(["ferrowl", "--device", "d.toml"]).is_ok());
    }

    #[test]
    /// CL-R-015 — --exit-on-error exists only on the run subcommand.
    fn ut_exit_on_error_is_run_only() {
        assert!(CliArgs::try_parse_from(["ferrowl", "--exit-on-error"]).is_err());
        assert!(CliArgs::try_parse_from(["ferrowl", "run", "--exit-on-error"]).is_ok());
    }

    #[test]
    /// CL-R-016 — top-level values supplied alongside a run subcommand do not reach the runner.
    fn ut_run_ignores_top_level_values() {
        let args = CliArgs::parse_from(["ferrowl", "--module", "name=m,device=d,port=1", "run"]);
        assert_eq!(args.modules, vec!["name=m,device=d,port=1".to_string()]);
        match args.command {
            Some(SubCommand::Run(run)) => {
                assert!(
                    run.modules.is_empty(),
                    "run must not see the top-level --module"
                );
            }
            _ => panic!("expected run"),
        }
    }

    #[test]
    /// CL-R-035 — an argument-parsing error exits with the parser's usage exit code (2).
    fn ut_arg_parse_error_exits_two() {
        use clap::error::ErrorKind;
        let e = CliArgs::try_parse_from(["ferrowl", "--bogus-flag"]).unwrap_err();
        assert_ne!(e.kind(), ErrorKind::DisplayHelp);
        assert_eq!(e.exit_code(), 2);
    }

    // --- Config envelope: module-entry type dispatch and required fields ------------------

    /// Save `modules` as a session file and resolve it through [`CliArgs::module_specs`].
    fn resolve_session(
        tag: &str,
        modules: Vec<serde_json::Value>,
    ) -> Result<Vec<ModuleSpec>, String> {
        use ferrowl_util::convert::{Converter, FileType};
        let session = config::Session {
            version: None,
            modules,
            scripts: vec![],
            interval: 1.0,
        };
        let path = std::env::temp_dir().join(format!("ferrowl_cli_{tag}.toml"));
        Converter::save(&session, path.to_str().unwrap(), FileType::Toml).unwrap();
        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec![path.to_str().unwrap().to_string()],
            devices: vec![],
            demo: false,
        };
        args.module_specs()
    }

    #[test]
    /// CS-R-013 — a module entry whose `type` is neither modbus nor ocpp aborts session resolution.
    fn ut_unknown_module_type_aborts_resolution() {
        let err = resolve_session(
            "unknown_type",
            vec![serde_json::json!({"type": "plc", "name": "x"})],
        )
        .unwrap_err();
        assert!(
            err.contains("unsupported module type"),
            "expected a hard error, got: {err}"
        );
    }

    #[test]
    /// CS-R-051 — a module instance missing a schema-required field fails to load.
    fn ut_module_missing_required_field_errors() {
        let mut module =
            serde_json::to_value(create_module_spec_by_device("x".into(), "d.toml".into()))
                .unwrap();
        // Drop the required `name`, keep the modbus tag.
        module.as_object_mut().unwrap().remove("name");
        module
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());
        assert!(resolve_session("missing_name", vec![module]).is_err());
    }

    // --- Bridge mode: SubCommand::Bridge and descriptor parsing ---------------------------

    #[test]
    /// BR-R-001, BR-R-003, BR-R-014 — the bridge subcommand parses upstream/downstream/
    /// duration flags.
    fn ut_bridge_subcommand_parses() {
        let args = CliArgs::parse_from([
            "ferrowl",
            "bridge",
            "--upstream",
            "transport=tcp,ip=0.0.0.0,port=502",
            "--downstream",
            "transport=tcp,ip=10.0.0.5,port=502",
            "--duration",
            "5",
        ]);
        match args.command {
            Some(SubCommand::Bridge(bridge)) => {
                assert_eq!(
                    bridge.upstream.as_deref(),
                    Some("transport=tcp,ip=0.0.0.0,port=502")
                );
                assert_eq!(
                    bridge.downstream.as_deref(),
                    Some("transport=tcp,ip=10.0.0.5,port=502")
                );
                assert_eq!(bridge.duration, Some(5));
            }
            _ => panic!("expected SubCommand::Bridge"),
        }
    }

    #[test]
    /// BR-R-003 — clap accepts `bridge` with neither --upstream nor --downstream; the
    /// required-endpoint check happens at runtime, not at the clap layer.
    fn ut_bridge_upstream_and_downstream_both_optional_at_clap_layer() {
        assert!(CliArgs::try_parse_from(["ferrowl", "bridge"]).is_ok());
    }

    #[test]
    /// BR-R-004 — a bare `port=502` descriptor defaults transport/ip/timeout/reconnect/tls/
    /// unit_ids.
    fn ut_parse_bridge_descriptor_tcp_defaults() {
        let spec = parse_bridge_descriptor("port=502").unwrap();
        assert!(spec.unit_ids.is_none());
        match spec.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::Tcp(cfg) => {
                assert_eq!(cfg.ip, "127.0.0.1");
                assert_eq!(cfg.port, 502);
                assert_eq!(cfg.timeout_ms, 3000);
                assert!(cfg.reconnect);
                assert!(cfg.tls.is_none());
            }
            _ => panic!("expected Tcp"),
        }
    }

    #[test]
    /// BR-R-004 — every rtu_over_tcp descriptor key round-trips, reusing the same field set
    /// (and defaults) as plain tcp.
    fn ut_parse_bridge_descriptor_rtu_over_tcp_full() {
        let spec = parse_bridge_descriptor(
            "transport=rtu_over_tcp,ip=10.0.0.9,port=1502,timeout_ms=500,reconnect=false",
        )
        .unwrap();
        match spec.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::RtuOverTcp(cfg) => {
                assert_eq!(cfg.ip, "10.0.0.9");
                assert_eq!(cfg.port, 1502);
                assert_eq!(cfg.timeout_ms, 500);
                assert!(!cfg.reconnect);
            }
            _ => panic!("expected RtuOverTcp"),
        }
    }

    #[test]
    /// BR-R-004 — a rtu_over_tcp descriptor missing `port` errors, same as plain tcp.
    fn ut_parse_bridge_descriptor_rtu_over_tcp_requires_port() {
        assert!(parse_bridge_descriptor("transport=rtu_over_tcp").is_err());
    }

    #[test]
    /// BR-R-004 — every ascii_over_tcp descriptor key round-trips, reusing the same field set
    /// (and defaults) as plain tcp.
    fn ut_parse_bridge_descriptor_ascii_over_tcp_full() {
        let spec = parse_bridge_descriptor(
            "transport=ascii_over_tcp,ip=10.0.0.10,port=1503,timeout_ms=500,reconnect=false",
        )
        .unwrap();
        match spec.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::AsciiOverTcp(cfg) => {
                assert_eq!(cfg.ip, "10.0.0.10");
                assert_eq!(cfg.port, 1503);
                assert_eq!(cfg.timeout_ms, 500);
                assert!(!cfg.reconnect);
            }
            _ => panic!("expected AsciiOverTcp"),
        }
    }

    #[test]
    /// BR-R-004 — an ascii_over_tcp descriptor missing `port` errors, same as plain tcp.
    fn ut_parse_bridge_descriptor_ascii_over_tcp_requires_port() {
        assert!(parse_bridge_descriptor("transport=ascii_over_tcp").is_err());
    }

    #[test]
    /// BR-R-011 — rtu_over_tcp/ascii_over_tcp descriptors accept the same opt-in `tls` field
    /// set as plain tcp (BR-R-004's shared tcp::Config field set).
    fn ut_parse_bridge_descriptor_over_tcp_variants_accept_tls() {
        for transport in ["rtu_over_tcp", "ascii_over_tcp"] {
            let spec = parse_bridge_descriptor(&format!(
                "transport={transport},port=502,self_signed=true"
            ))
            .unwrap();
            let tls = match spec.kind {
                ferrowl_modbus::bridge::BridgeEndpointKind::RtuOverTcp(cfg) => cfg.tls,
                ferrowl_modbus::bridge::BridgeEndpointKind::AsciiOverTcp(cfg) => cfg.tls,
                _ => panic!("expected an over-tcp variant"),
            };
            assert!(tls.is_some(), "{transport} must accept tls fields");
        }
    }

    #[test]
    /// BR-R-004, BR-R-015 — every rtu descriptor key round-trips.
    fn ut_parse_bridge_descriptor_rtu_full() {
        let spec = parse_bridge_descriptor(
            "transport=rtu,path=/dev/ttyUSB0,baud=9600,parity=even,data_bits=7,stop_bits=2,\
             timeout_ms=500,reconnect=false,unit_ids=1,3",
        )
        .unwrap();
        assert_eq!(
            spec.unit_ids,
            Some(ferrowl_modbus::bridge::UnitIdFilter::parse("1,3").unwrap())
        );
        match spec.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::Rtu(cfg) => {
                assert_eq!(cfg.path, "/dev/ttyUSB0");
                assert_eq!(cfg.baud_rate, 9600);
                assert_eq!(cfg.parity.as_deref(), Some("even"));
                assert_eq!(cfg.data_bits, Some(7));
                assert_eq!(cfg.stop_bits, Some(2));
                assert_eq!(cfg.timeout_ms, 500);
                assert!(!cfg.reconnect);
            }
            _ => panic!("expected Rtu"),
        }
    }

    #[test]
    /// BR-R-011 — `tls` stays `None` unless at least one tls key is present.
    fn ut_parse_bridge_descriptor_tls_opt_in() {
        let no_tls = parse_bridge_descriptor("port=502").unwrap();
        match no_tls.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::Tcp(cfg) => assert!(cfg.tls.is_none()),
            _ => unreachable!(),
        }

        let with_tls = parse_bridge_descriptor("port=502,self_signed=true").unwrap();
        match with_tls.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::Tcp(cfg) => {
                let tls = cfg.tls.expect("tls present");
                assert_eq!(
                    tls.server,
                    ferrowl_util::tls::ServerTlsPolicy::Tls {
                        server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned
                    }
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    /// MB-R-106 — via the CLI mini-language: `self_signed=true` wins unconditionally over
    /// `cert_file`/`key_file` present in the same descriptor.
    fn ut_parse_bridge_descriptor_tls_self_signed_wins_over_cert_files() {
        let parsed =
            parse_bridge_descriptor("port=502,self_signed=true,cert_file=s.crt,key_file=s.key")
                .unwrap();
        match parsed.kind {
            ferrowl_modbus::bridge::BridgeEndpointKind::Tcp(cfg) => {
                let tls = cfg.tls.expect("tls present");
                assert_eq!(
                    tls.server,
                    ferrowl_util::tls::ServerTlsPolicy::Tls {
                        server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned
                    }
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    /// MB-R-107 — via the CLI mini-language: `cert_file` set alone (no `self_signed`, no
    /// `key_file`) is a configuration-resolution error.
    fn ut_parse_bridge_descriptor_tls_cert_file_alone_is_error() {
        assert!(parse_bridge_descriptor("port=502,cert_file=s.crt").is_err());
    }

    #[test]
    /// BR-R-004 — an unknown transport, a tcp descriptor missing `port`, and an rtu descriptor
    /// missing `path` all error.
    fn ut_parse_bridge_descriptor_rejects_invalid_transport_and_missing_required() {
        assert!(parse_bridge_descriptor("transport=usb,port=502").is_err());
        assert!(parse_bridge_descriptor("transport=tcp").is_err());
        assert!(parse_bridge_descriptor("transport=rtu").is_err());
    }

    #[test]
    /// BR-R-004 — an invalid `reconnect` value errors.
    fn ut_parse_bridge_descriptor_rejects_bad_reconnect_value() {
        assert!(parse_bridge_descriptor("port=502,reconnect=maybe").is_err());
    }
}
