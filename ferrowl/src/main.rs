//! ferrowl — a terminal UI for inspecting and simulating Modbus devices.
//!
//! Each configured module (a device definition plus a TCP/RTU endpoint and
//! client/server role) runs as a background task and is shown as a tab in
//! the TUI. Entry point: parse CLI args, build the tabs, then hand control
//! to [`App::run`].

mod app;
mod cli;
mod command;
mod config;
mod dialog;
mod instance;
mod lua;
mod migrate;
mod module;
mod registry;
mod script_template;
mod session;
mod view;

use std::collections::BTreeMap;
use std::io::Stdout;

use clap::Parser;
use ferrowl_codec::Kind;
use ferrowl_ui::AlternateScreen;
use ferrowl_util::Expect;
use tokio::runtime::Runtime;

use crate::app::{App, Level, Tab};
use crate::cli::{CliArgs, SubCommand};
use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, NamedValue, RegisterDef, Scalar, ValueType, WordOrderCfg,
};
use crate::config::ocpp::{OcppProtocol, OcppRole, OcppVersion};
use crate::config::{
    DeviceConfig, Endpoint, ModuleSpec, OcppDeviceConfig, OcppModuleSpec, OcppSpec, Role,
};
use crate::module::modbus::ModbusModule as Module;
use crate::module::modbus::view::ModbusModuleView;
use crate::module::modbus::{ModbusMonitorModule as MonitorModule, ModbusMonitorModuleView};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::server::build_server_view;
use crate::module::view::{CommandResult, ModuleView};

/// In-code demo module shown when no `--module`/`--session` is given (not started, so it
/// binds nothing).
fn demo_modbus_tab(name: String, role: Role) -> Tab {
    let reg = |address: u16, value_type: ValueType, description: &str, values: Vec<NamedValue>| {
        RegisterDef {
            slave_id: 1,
            kind: Kind::HoldingRegister,
            address: Some(address),
            is_virtual: false,
            access: AccessCfg::ReadWrite,
            value_type,
            endian: EndianCfg::Big,
            word_order: WordOrderCfg::default(),
            resolution: 1.0,
            bitmask: None,
            length: 1,
            alignment: AlignmentCfg::Left,
            values,
            update: None,
            description: description.to_string(),
            default: None,
        }
    };

    let mut definitions = BTreeMap::new();
    definitions.insert(
        "SetPoint".to_string(),
        reg(0, ValueType::U16, "Limit set by external control.", vec![]),
    );
    definitions.insert(
        "Power".to_string(),
        reg(1, ValueType::U16, "Active power consumption.", vec![]),
    );
    definitions.insert(
        "Current L1".to_string(),
        reg(2, ValueType::I16, "Active current on L1", vec![]),
    );
    definitions.insert(
        "Current L2".to_string(),
        reg(3, ValueType::I16, "Active current on L2", vec![]),
    );
    definitions.insert(
        "Current L3".to_string(),
        reg(4, ValueType::I16, "Active current on L3.", vec![]),
    );
    definitions.insert(
        "Mode".to_string(),
        reg(
            5,
            ValueType::I16,
            "Active mode.",
            vec![
                NamedValue {
                    name: "Idle".into(),
                    value: Scalar::Int(10),
                },
                NamedValue {
                    name: "Present".into(),
                    value: Scalar::Int(11),
                },
                NamedValue {
                    name: "Charging".into(),
                    value: Scalar::Int(12),
                },
            ],
        ),
    );

    let device = DeviceConfig {
        version: None,
        timeout_ms: None,
        delay_ms: None,
        interval_ms: None,
        reconnect: None,
        tls: Default::default(),
        log_file: None,
        read_ranges: Default::default(),
        definitions,
        scripts: Vec::new(),
        script_interval: 1.0,
    };
    let spec = ModuleSpec {
        name,
        device: String::new(),
        role,
        endpoint: Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 5020,
        },
    };

    let module = Module::new(&spec, &device);
    let view: Box<dyn ModuleView> = Box::new(ModbusModuleView::new(module, spec.clone(), device));
    Tab::new_from_view(spec.name.clone(), view)
}

fn demo_ocpp_tab(name: String, version: OcppVersion, role: OcppRole, port: u16) -> Tab {
    let spec = OcppSpec {
        name,
        version,
        role,
        protocol: OcppProtocol::Ws,
        ip: "127.0.0.1".into(),
        port,
        path: "/ocpp/cp001".into(),
        timeout_ms: None,
        reconnect: None,
        security: Default::default(),
    };
    let device = OcppDeviceConfig::from_spec(&spec, Vec::new());
    let view = match role {
        OcppRole::Client => build_client_view(spec.clone(), String::new(), device),
        OcppRole::Server => build_server_view(spec.clone(), String::new(), device),
    };
    Tab::new_from_view(spec.name.clone(), view)
}

/// The demo session's example script: the bundled `power-report` template (SC-R-036), which uses
/// `C_Module` to read the `Power` value of every modbus server instance and every OCPP connector,
/// printing the total to the session log once per cycle.
fn demo_session_script() -> crate::config::script::ScriptDef {
    let template = script_template::by_name("power-report")
        .expect("the bundled power-report session template");
    crate::config::script::ScriptDef {
        name: template.name.to_string(),
        code: template.code.to_string(),
        enabled: true,
    }
}

/// Resolves the session-level Lua scripts + sim interval from every `--session <file>` passed on
/// the command line: scripts concatenate across files (in order), and the interval is the last
/// file's (matching how later `--session` files already win module-spec conflicts nowhere in
/// particular — there's no real precedent here, so "last wins" is the simplest rule). A file that
/// fails to load is silently skipped: `build_tabs` already validated every session file's
/// loadability before this runs, so a failure here only happens if the file vanished mid-startup,
/// in which case falling back to the 1s default is preferable to aborting startup a second time.
fn session_sim_config(
    paths: &[String],
) -> (Vec<crate::config::script::ScriptDef>, std::time::Duration) {
    let mut scripts = Vec::new();
    let mut interval = std::time::Duration::from_secs_f64(1.0);
    for path in paths {
        if let Ok(session) = config::load_session(path) {
            interval = session.interval_duration();
            scripts.extend(session.scripts);
        }
    }
    (scripts, interval)
}

/// Builds one UI tab per configured module, starting each module via `handle_command("start")`.
/// Start failures are written to the module's log. Modules whose device config fails to load
/// are skipped with a warning on stderr.
async fn build_tabs(args: &CliArgs) -> Result<Vec<Tab>, String> {
    let specs = args.module_specs()?;

    if args.demo {
        let mut tabs = vec![
            demo_modbus_tab("Modbus Server".to_string(), Role::Server),
            demo_modbus_tab("Modbus Client".to_string(), Role::Client),
            demo_ocpp_tab(
                "CSMS v1.6".to_string(),
                OcppVersion::V1_6,
                OcppRole::Server,
                9000,
            ),
            demo_ocpp_tab(
                "CS v1.6".to_string(),
                OcppVersion::V1_6,
                OcppRole::Client,
                9000,
            ),
            demo_ocpp_tab(
                "CSMS v2.0.1".to_string(),
                OcppVersion::V2_0_1,
                OcppRole::Server,
                9001,
            ),
            demo_ocpp_tab(
                "CS v2.0.1".to_string(),
                OcppVersion::V2_0_1,
                OcppRole::Client,
                9001,
            ),
            demo_ocpp_tab(
                "CSMS v2.1".to_string(),
                OcppVersion::V2_1,
                OcppRole::Server,
                9002,
            ),
            demo_ocpp_tab(
                "CS v2.1".to_string(),
                OcppVersion::V2_1,
                OcppRole::Client,
                9002,
            ),
        ];
        for tab in tabs.iter_mut() {
            if let CommandResult::Handled(Some((level, msg))) =
                tab.view.handle_command("start").await
            {
                tab.log.write().await.write(level, &msg);
            }
        }
        return Ok(tabs);
    }

    // Resolve every tab's final name up front (across both module types, in creation order) so a
    // session with duplicate module names still gets distinct tabs instead of silently colliding
    // in the `C_Module` registry: repeats are auto-suffixed " (2)", " (3)", ...
    let ocpp_specs = args.ocpp_specs()?;
    let mut names: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();
    names.extend(ocpp_specs.iter().map(|s| s.name.clone()));
    let resolved = crate::registry::dedupe_names(&names);

    let mut tabs = Vec::new();
    for (spec, resolved_name) in specs.iter().zip(&resolved) {
        let mut spec = spec.clone();
        let renamed = *resolved_name != spec.name;
        spec.name = resolved_name.clone();

        let view: Box<dyn ModuleView> = match spec.role {
            Role::Monitor => {
                let device = match config::load_monitor_device(&spec.device) {
                    Ok(device) => device,
                    Err(e) => {
                        eprintln!(
                            "Skipping '{}': failed to load '{}': {e}",
                            spec.name, spec.device
                        );
                        continue;
                    }
                };
                let module = MonitorModule::new(&spec, &device);
                Box::new(ModbusMonitorModuleView::new(module, spec.clone(), device))
            }
            Role::Client | Role::Server => {
                let device = match config::load_device(&spec.device) {
                    Ok(device) => device,
                    Err(e) => {
                        eprintln!(
                            "Skipping '{}': failed to load '{}': {e}",
                            spec.name, spec.device
                        );
                        continue;
                    }
                };
                let module = Module::new(&spec, &device);
                Box::new(ModbusModuleView::new(module, spec.clone(), device))
            }
        };
        let mut tab = Tab::new_from_view(spec.name.clone(), view);
        if renamed {
            tab.log.write().await.write(
                Level::Warning,
                &format!(
                    "Warning: duplicate module name — renamed to '{}'",
                    spec.name
                ),
            );
        }
        if let CommandResult::Handled(Some((level, msg))) = tab.view.handle_command("start").await {
            tab.log.write().await.write(level, &msg);
        }
        tabs.push(tab);
    }

    for (spec, resolved_name) in ocpp_specs.into_iter().zip(&resolved[specs.len()..]) {
        // A blank path is a legitimate quick-start (no device file, run on defaults). A non-blank
        // path that fails to load is a real config error and is skipped with a warning, exactly as
        // a Modbus module would be -- rather than silently starting a default-config tab that hides
        // the mistake.
        let device = if spec.device.is_empty() {
            OcppDeviceConfig::default()
        } else {
            match config::load_ocpp_device(&spec.device) {
                Ok(device) => device,
                Err(e) => {
                    eprintln!(
                        "Skipping '{}': failed to load '{}': {e}",
                        spec.name, spec.device
                    );
                    continue;
                }
            }
        };
        let renamed = *resolved_name != spec.name;
        let mut spec = spec;
        spec.name = resolved_name.clone();
        let tab = build_ocpp_tab(spec, device).await;
        if renamed {
            tab.log.write().await.write(
                Level::Warning,
                &format!("Warning: duplicate module name — renamed to '{}'", tab.name),
            );
        }
        tabs.push(tab);
    }
    Ok(tabs)
}

/// Build a UI tab for one OCPP module spec and its already-loaded device config, starting it via
/// `handle_command("start")`. Loading (and the skip-on-failure decision) happens in the caller, so
/// a missing device file is handled the same way as for a Modbus module.
async fn build_ocpp_tab(module: OcppModuleSpec, device: OcppDeviceConfig) -> Tab {
    let name = module.name.clone();
    let spec = OcppSpec::from_parts(&module, &device);
    let view: Box<dyn ModuleView> = match device.role {
        OcppRole::Client => build_client_view(spec, module.device.clone(), device),
        OcppRole::Server => build_server_view(spec, module.device.clone(), device),
    };
    let mut tab = Tab::new_from_view(name, view);
    if let CommandResult::Handled(Some((level, msg))) = tab.view.handle_command("start").await {
        tab.log.write().await.write(level, &msg);
    }
    tab
}

fn main() {
    let args = CliArgs::parse();

    if let Some(SubCommand::Migrate(ref migrate_args)) = args.command {
        migrate::run(migrate_args);
        return;
    }

    // Multi-threaded runtime: background modbus/OCPP tasks run concurrently with the UI loop.
    let runtime = Runtime::new().panic(|e| format!("Failed to create runtime. [{e}]"));

    if let Some(SubCommand::Run(ref run_args)) = args.command {
        let code = runtime.block_on(cli::headless::run(run_args));
        std::process::exit(code);
    }

    if let Some(SubCommand::Bridge(ref bridge_args)) = args.command {
        let code = runtime.block_on(cli::bridge::run(bridge_args));
        std::process::exit(code);
    }

    // Release the terminal on panic so the error message is visible.
    let handler = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        AlternateScreen::<Stdout>::release();
        handler(panic);
    }));

    runtime.block_on(async move {
        let tabs = match build_tabs(&args).await {
            Ok(tabs) => tabs,
            Err(e) => {
                eprintln!("Error: {e}");
                return;
            }
        };
        let (session_scripts, session_interval) = if args.demo {
            // The demo session ships an example session script showcasing `C_Module`.
            (
                vec![demo_session_script()],
                std::time::Duration::from_secs_f64(1.0),
            )
        } else {
            session_sim_config(&args.sessions)
        };

        let mut app = match App::new(tabs, session_scripts, session_interval) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("Failed to set up screen: {e}");
                return;
            }
        };
        if let Err(e) = app.run().await {
            AlternateScreen::<Stdout>::release();
            eprintln!("UI error: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{build_tabs, demo_session_script};
    use crate::cli::{CliArgs, create_module_spec_by_device};
    use crate::config::{self, OcppModuleSpec, Session};
    use crate::registry::ModuleRegistry;
    use ferrowl_lua::ContextBuilder;
    use ferrowl_lua::module::ModuleDirModule;
    use ferrowl_test_support::{TempDirGuard, reserve_tcp_port, reserve_temp_dir};
    use ferrowl_util::convert::{Converter, FileType};
    use std::sync::Arc;

    fn demo_args(modules: Vec<String>, devices: Vec<String>) -> CliArgs {
        CliArgs {
            command: None,
            modules,
            sessions: vec![],
            devices,
            demo: true,
        }
    }

    /// Saves `session` under a fresh temp dir, returning its path and the guard keeping the
    /// dir alive for the rest of the test.
    fn save_session(tag: &str, session: &Session) -> (String, TempDirGuard) {
        let dir = reserve_temp_dir(&format!("ferrowl_main_{tag}"));
        let path = dir.join("session.toml");
        Converter::save(session, path.to_str().unwrap(), FileType::Toml).unwrap();
        (path.to_str().unwrap().to_string(), dir)
    }

    // The demo session script must at least parse and run cleanly against an empty module
    // registry (no modules -> the loop body never executes, so C_Log is never touched).
    #[test]
    /// SC-R-020 — the demo session-level script runs against the module registry.
    fn ut_demo_session_script_runs_on_empty_registry() {
        let script = demo_session_script();
        assert!(script.enabled);
        let registry = ModuleRegistry::new();
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(ModuleDirModule::init(Arc::new(registry)))
            .with_script(script.name.clone(), &script.code)
            .build()
            .expect("demo session script must parse");
        ctx.call_all()
            .expect("demo session script must run on an empty registry");
    }

    #[tokio::test]
    /// CL-R-006 — --demo starts exactly eight tabs (two modbus, six ocpp) plus one example
    /// session-level script.
    async fn ut_demo_builds_eight_tabs_and_a_session_script() {
        let tabs = build_tabs(&demo_args(vec![], vec![])).await.unwrap();
        assert_eq!(tabs.len(), 8, "the demo set is exactly eight tabs");
        assert!(
            !demo_session_script().code.is_empty(),
            "the demo session ships one example script"
        );
    }

    #[tokio::test]
    /// CL-R-005 — under --demo, --module/--session/--device are ignored for building tabs.
    async fn ut_demo_ignores_module_and_device() {
        let args = demo_args(
            vec!["name=x,device=d.toml,port=1".into()],
            vec!["dev.toml".into()],
        );
        let tabs = build_tabs(&args).await.unwrap();
        assert_eq!(
            tabs.len(),
            8,
            "demo ignores --module/--device: still exactly the demo set"
        );
    }

    #[tokio::test]
    /// CS-R-053 — a session instance whose device config is missing is skipped, not fatal.
    async fn ut_missing_device_is_skipped_not_fatal() {
        let mut module = serde_json::to_value(create_module_spec_by_device(
            "mb".into(),
            "/no/such/dev.toml".into(),
        ))
        .unwrap();
        module
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());
        let session = Session {
            version: None,
            modules: vec![module],
            scripts: vec![],
            interval: 1.0,
        };
        let (session_path, _session_dir) = save_session("skip", &session);
        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec![session_path],
            devices: vec![],
            demo: false,
        };
        let tabs = build_tabs(&args).await.unwrap();
        assert!(tabs.is_empty(), "the missing-device instance is skipped");
    }

    #[tokio::test]
    /// CS-R-053 — a blank device path is a quick-start on the default device, built not skipped.
    async fn ut_blank_device_builds_on_default() {
        let mut module = serde_json::to_value(OcppModuleSpec {
            name: "cs".into(),
            device: String::new(),
            protocol: config::ocpp::OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: reserve_tcp_port().release(),
            path: String::new(),
        })
        .unwrap();
        module
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "ocpp".into());
        let session = Session {
            version: None,
            modules: vec![module],
            scripts: vec![],
            interval: 1.0,
        };
        let (session_path, _session_dir) = save_session("blank", &session);
        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec![session_path],
            devices: vec![],
            demo: false,
        };
        let tabs = build_tabs(&args).await.unwrap();
        assert_eq!(
            tabs.len(),
            1,
            "a blank device path builds on the default config"
        );
    }

    #[tokio::test]
    /// MB-R-140/MB-R-141 — a session module with `role = "monitor"` loads and starts a working
    /// tab: `MonitorBuilder::spawn` always returns `Ok` (the serial open/retry happen inside the
    /// spawned task), so a monitor module on a non-existent serial path still builds one tab,
    /// mirroring `it_headless_run_starts_monitor_module` (`tests/headless.rs`) for the TUI's own
    /// `build_tabs` construction path.
    async fn ut_session_with_monitor_module_loads_and_starts() {
        use crate::config::{Endpoint, ModuleSpec, MonitorDeviceConfig, Role};

        let device_dir = reserve_temp_dir("ferrowl_main_monitor");
        let device_path = device_dir.join("device.toml");
        Converter::save(
            &MonitorDeviceConfig::default(),
            device_path.to_str().unwrap(),
            FileType::Toml,
        )
        .unwrap();

        let spec = ModuleSpec {
            name: "mon".into(),
            device: device_path.to_str().unwrap().to_string(),
            role: Role::Monitor,
            endpoint: Endpoint::Rtu {
                path: "/dev/ttyNONE-ut-main-monitor".to_string(),
                baud_rate: 9600,
                parity: None,
                data_bits: Some(8),
                stop_bits: Some(1),
            },
        };
        let mut module = serde_json::to_value(spec).unwrap();
        module
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());
        let session = Session {
            version: None,
            modules: vec![module],
            scripts: vec![],
            interval: 1.0,
        };
        let (session_path, _session_dir) = save_session("monitor-ok", &session);
        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec![session_path],
            devices: vec![],
            demo: false,
        };
        let tabs = build_tabs(&args).await.unwrap();
        assert_eq!(tabs.len(), 1, "the monitor module builds one working tab");
    }

    #[tokio::test]
    /// MB-R-140 — a monitor module referencing a device file that fails `load_monitor_device` is
    /// skipped (not fatal), same as today's `load_device` failure for Client/Server (see
    /// `ut_missing_device_is_skipped_not_fatal` above), mirroring
    /// `it_headless_fails_on_monitor_module_with_bad_device_path` (`tests/headless.rs`) — the TUI
    /// path skips-with-a-warning rather than hard-failing the whole run.
    async fn ut_session_skips_monitor_module_with_bad_device_path() {
        use crate::config::{Endpoint, ModuleSpec, Role};

        let spec = ModuleSpec {
            name: "mon-bad".into(),
            device: "/no/such/monitor-device.toml".to_string(),
            role: Role::Monitor,
            endpoint: Endpoint::Rtu {
                path: "/dev/ttyNONE-ut-main-monitor-bad".to_string(),
                baud_rate: 9600,
                parity: None,
                data_bits: Some(8),
                stop_bits: Some(1),
            },
        };
        let mut module = serde_json::to_value(spec).unwrap();
        module
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());
        let session = Session {
            version: None,
            modules: vec![module],
            scripts: vec![],
            interval: 1.0,
        };
        let (session_path, _session_dir) = save_session("monitor-bad-device", &session);
        let args = CliArgs {
            command: None,
            modules: vec![],
            sessions: vec![session_path],
            devices: vec![],
            demo: false,
        };
        let tabs = build_tabs(&args).await.unwrap();
        assert!(
            tabs.is_empty(),
            "the monitor instance with a bad device path is skipped"
        );
    }
}
