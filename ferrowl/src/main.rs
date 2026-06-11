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
use ferrowl_ui::AlternateScreen;
use ferrowl_util::Expect;
use tokio::runtime::Runtime;

use crate::app::{App, Tab};
use crate::cli::{CliArgs, SubCommand};
use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, NamedValue, RegisterDef, Scalar, ValueType,
};
use crate::config::{DeviceConfig, Endpoint, ModuleSpec, Role};
use crate::module::Module;

/// In-code demo module shown when no `--module`/`--session` is given (not started, so it
/// binds nothing).
fn demo() -> (DeviceConfig, ModuleSpec) {
    let reg = |address: u16, value_type: ValueType, description: &str, values: Vec<NamedValue>| {
        RegisterDef {
            slave_id: 1,
            read_code: 4,
            address: Some(address),
            is_virtual: false,
            access: AccessCfg::ReadWrite,
            value_type,
            endian: EndianCfg::Big,
            resolution: 1.0,
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
    };
    let spec = ModuleSpec {
        name: "demo".to_string(),
        device: String::new(),
        role: Role::Server,
        endpoint: Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 5020,
        },
        timeout_ms: None,
        delay_ms: None,
        interval_ms: None,
    };
    (device, spec)
}

/// Builds one UI tab per configured module, starting each module's
/// background task. Start failures are written to the module's log; modules
/// whose device config fails to load are skipped with a warning on stderr.
async fn build_tabs(args: &CliArgs) -> Result<Vec<Tab>, String> {
    let specs = args.module_specs()?;

    if args.demo {
        let (device, spec) = demo();
        let mut module = Module::new(&spec, &device);
        let role = spec.role;

        if let Err(e) = module.start().await {
            module
                .log()
                .write()
                .await
                .write(&format!("Failed to start {role}: {e}"));
        } else {
            module
                .log()
                .write()
                .await
                .write(&format!("Started {role} on {}", spec.endpoint));
        }

        return Ok(vec![Tab::from_module(spec, device, module)]);
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
        let role = spec.role;
        let mut module = Module::new(spec, &device);
        // CLI/session modules auto-start; start failures are surfaced into the module log.
        if let Err(e) = module.start().await {
            module
                .log()
                .write()
                .await
                .write(&format!("Failed to start {role}: {e}"));
        } else {
            module
                .log()
                .write()
                .await
                .write(&format!("Started {role} on {}", spec.endpoint));
        }
        tabs.push(Tab::from_module(spec.clone(), device, module));
    }
    Ok(tabs)
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
