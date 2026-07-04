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
mod view;

use std::collections::BTreeMap;
use std::io::Stdout;

use clap::Parser;
use ferrowl_codec::Kind;
use ferrowl_ui::AlternateScreen;
use ferrowl_util::Expect;
use tokio::runtime::Runtime;

use crate::app::{App, Tab};
use crate::cli::{CliArgs, SubCommand};
use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, NamedValue, RegisterDef, Scalar, ValueType,
};
use crate::config::ocpp::{OcppProtocol, OcppRole, OcppVersion};
use crate::config::{
    DeviceConfig, Endpoint, ModuleSpec, OcppDeviceConfig, OcppModuleSpec, OcppSpec, Role,
};
use crate::module::modbus::ModbusModule as Module;
use crate::module::modbus::view::ModbusModuleView;
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
        log_file: None,
        read_ranges: Default::default(),
        definitions,
        scripts: Vec::new(),
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
    };
    let device = OcppDeviceConfig::from_spec(&spec, Vec::new());
    let view = match role {
        OcppRole::Client => build_client_view(spec.clone(), String::new(), device),
        OcppRole::Server => build_server_view(spec.clone(), String::new(), device),
    };
    Tab::new_from_view(spec.name.clone(), view)
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
            if let CommandResult::Handled(Some(msg)) = tab.view.handle_command("start").await {
                tab.log.write().await.write(&msg);
            }
        }
        return Ok(tabs);
    }

    let mut tabs = Vec::new();
    for spec in &specs {
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
        let module = Module::new(spec, &device);
        let view: Box<dyn ModuleView> =
            Box::new(ModbusModuleView::new(module, spec.clone(), device));
        let mut tab = Tab::new_from_view(spec.name.clone(), view);
        if let CommandResult::Handled(Some(msg)) = tab.view.handle_command("start").await {
            tab.log.write().await.write(&msg);
        }
        tabs.push(tab);
    }

    for spec in args.ocpp_specs()? {
        tabs.push(build_ocpp_tab(spec).await);
    }
    Ok(tabs)
}

/// Build a UI tab for one OCPP module spec, starting it via `handle_command("start")`. The
/// device-config file (role/version/timeout/scripts) is loaded from the spec's path; a missing or
/// unreadable file falls back to defaults.
async fn build_ocpp_tab(module: OcppModuleSpec) -> Tab {
    let name = module.name.clone();
    let device = config::load_ocpp_device(&module.device).unwrap_or_default();
    let spec = OcppSpec::from_parts(&module, &device);
    let view: Box<dyn ModuleView> = match device.role {
        OcppRole::Client => build_client_view(spec, module.device.clone(), device),
        OcppRole::Server => build_server_view(spec, module.device.clone(), device),
    };
    let mut tab = Tab::new_from_view(name, view);
    if let CommandResult::Handled(Some(msg)) = tab.view.handle_command("start").await {
        tab.log.write().await.write(&msg);
    }
    tab
}

fn main() {
    let args = CliArgs::parse();

    if let Some(SubCommand::Migrate(ref migrate_args)) = args.command {
        migrate::run(migrate_args);
        return;
    }

    // Multi-threaded runtime so background modbus tasks can use `block_in_place`.
    let runtime = Runtime::new().panic(|e| format!("Failed to create runtime. [{}]", e));

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

        let mut app = match App::new(tabs) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("Failed to set up screen: {e}");
                return;
            }
        };
        if let Err(e) = app.run().await {
            AlternateScreen::<Stdout>::release();
            eprintln!("UI error: {}", e);
        }
    });
}
