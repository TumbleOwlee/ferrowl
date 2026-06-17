//! Execution of `:` commands against the active tab: module lifecycle, value writes,
//! ordering and persistence.

use ferrowl_codec::{Access, Address};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::Range;
use ferrowl_ui::widgets::Header;
use ferrowl_util::convert::{Converter, FileType};

use crate::config::{Role, Session};
use crate::module::Module;
use crate::view::main::TableHeader;
use crate::view::main::{Definition, column_index};

use super::registers::write_command;
use super::{App, Tab};

impl App {
    /// Execute a parsed `:` command. Returns `true` when the app should quit.
    pub(super) async fn run_command(&mut self, input: &str) -> bool {
        use crate::command::{Cmd, LuaCommand};
        match crate::command::parse(input) {
            Cmd::Empty => {}
            Cmd::Quit => {
                if self.tabs.len() <= 1 {
                    return true;
                }
                let _ = self.tabs[self.active].module.stop().await;
                self.tabs.remove(self.active);
                self.active = self.active.min(self.tabs.len() - 1);
            }
            Cmd::QuitAll => return true,
            Cmd::Edit => self.enter_setup(),
            Cmd::Add => self.enter_add(),
            Cmd::New => self.enter_new(),
            Cmd::Load(path) => self.enter_load(path.as_deref()),
            Cmd::Start => self.start_module().await,
            Cmd::Stop => self.stop_module().await,
            Cmd::Restart => {
                self.stop_module().await;
                self.start_module().await;
            }
            Cmd::Lua(action) => {
                let msg = if let Some(tab) = self.tabs.get_mut(self.active) {
                    match action {
                        LuaCommand::Start => {
                            tab.module.start_lua();
                            if tab.module.lua_running() {
                                "Lua simulation started".to_string()
                            } else {
                                "No Lua scripts to run".to_string()
                            }
                        }
                        LuaCommand::Stop => {
                            tab.module.stop_lua();
                            "Lua simulation stopped".to_string()
                        }
                        LuaCommand::Status => {
                            let running = tab.module.lua_running();
                            if running {
                                "Lua simulation is running".to_string()
                            } else {
                                "Lua simulation is stopped".to_string()
                            }
                        }
                    }
                } else {
                    return false;
                };
                self.log_active(format!("{}", msg)).await;
            }
            Cmd::Set { register, value } => {
                if register.is_empty() || value.is_empty() {
                    self.log_active(":set requires <register> <value>".to_string())
                        .await;
                } else {
                    self.set_value(&register, &value).await;
                }
            }
            Cmd::Write(path) => {
                let path = path.unwrap_or_else(|| "session.toml".to_string());
                match self.save_session(&path) {
                    Ok(()) => self.log_active(format!("Saved session to {path}")).await,
                    Err(e) => self.log_active(format!("Save failed: {e}")).await,
                }
            }
            Cmd::WriteDevice(path) => {
                const DEFAULT_PATH: &str = "device.toml";
                let path = if let Some(p) = &path {
                    p
                } else {
                    match self.tabs.get(self.active) {
                        Some(t) if !t.spec.device.is_empty() => &t.spec.device,
                        Some(t) if t.spec.device.is_empty() => {
                            self.log_active("No configuration file path configured.".to_string())
                                .await;
                            return false;
                        }
                        _ => DEFAULT_PATH,
                    }
                };
                match self.save_device(path) {
                    Ok(()) => {
                        self.log_active(format!("Saved device config to {path}"))
                            .await
                    }
                    Err(e) => self.log_active(format!("Save failed: {e}")).await,
                }
            }
            Cmd::Log(file) => match file {
                Some(file) => {
                    if file == "clear" {
                        if let Some(tab) = self.tabs.get(self.active) {
                            tab.module.log().write().await.clear();
                        }
                    } else if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.device.log_file = Some(file.clone());
                        tab.module.set_log_base(Some(&file));
                        self.log_active(format!(
                            "Logging this tab to files based on {file} (':wd' to persist)"
                        ))
                        .await;
                    }
                }
                None => self.log_active(":log requires <file>".to_string()).await,
            },
            Cmd::Compact => self.toggle_compact(),
            Cmd::Reload => self.reload_module().await,
            Cmd::Order { column, descending } => {
                self.set_order(column.as_deref(), descending).await
            }
            Cmd::Unknown(name) => self.log_active(format!("Unknown command ':{name}'")).await,
        }
        false
    }

    /// Apply (or clear) the active tab's table ordering for `:order`.
    async fn set_order(&mut self, column: Option<&str>, descending: bool) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        match column {
            None => {
                let original = self.tabs[active]
                    .module
                    .registers()
                    .iter()
                    .map(|(name, description, register, values)| {
                        Definition::new(
                            name.clone(),
                            description.clone(),
                            register.clone(),
                            values.clone(),
                        )
                    })
                    .collect();
                let tab = &mut self.tabs[active];
                tab.sort = None;
                tab.table.set_definitions(original);
                self.log_active("Order cleared".to_string()).await;
            }
            Some(name) => match column_index(name) {
                None => self.log_active(format!("Unknown column '{name}'")).await,
                Some(idx) => {
                    let tab = &mut self.tabs[active];
                    tab.sort = Some((idx, descending));
                    tab.table.sort_definitions(idx, descending);
                    let header = TableHeader::header()[idx].clone();
                    let dir = if descending { "DESC" } else { "ASC" };
                    self.log_active(format!("Ordered by {header} {dir}")).await;
                }
            },
        }
    }

    async fn reload_module(&mut self) {
        let active = self.active;
        let Some(tab) = self.tabs.get(active) else {
            return;
        };
        if tab.spec.device.is_empty() {
            self.log_active("No configuration file path configured. Reload aborted.".to_string())
                .await;
            return;
        }
        let path = tab.spec.device.clone();
        let device = match crate::config::load_device(&path) {
            Ok(d) => d,
            Err(e) => {
                self.log_active(format!(":reload failed to load '{path}': {e}"))
                    .await;
                return;
            }
        };
        let _ = self.tabs[active].module.stop().await;
        let mut module = Module::new(&self.tabs[active].spec, &device);
        if let Err(e) = module.start().await {
            self.log_active(format!(":reload start error: {e}")).await;
        }
        let spec = self.tabs[active].spec.clone();
        let new_tab = Tab::from_module(spec, device, module);
        let log_view = std::mem::replace(&mut self.tabs[active].log_view, new_tab.log_view);
        self.tabs[active] = Tab {
            log_view,
            ..new_tab
        };
        self.log_active(format!(":reload done — '{path}'")).await;
    }

    pub(super) async fn start_module(&mut self) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        let role = self.tabs[active].spec.role.to_string();
        let result = self.tabs[active].module.start().await;
        let msg = match result {
            Ok(()) => format!("Started {role} on {}", self.tabs[active].spec.endpoint),
            Err(e) => format!("Start {role} failed: {e}"),
        };
        self.tabs[active].module.log().write().await.write(&msg);
    }

    async fn stop_module(&mut self) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        let role = self.tabs[active].spec.role.to_string();
        let result = self.tabs[active].module.stop().await;
        let msg = match result {
            Ok(()) => format!("Stopped {role}"),
            Err(e) => format!("Stop {role} failed: {e}"),
        };
        self.tabs[active].module.log().write().await.write(&msg);
    }

    /// Write a value to a register on the active module: local memory for servers, a modbus
    /// write command for clients.
    pub(super) async fn set_value(&mut self, register_name: &str, value: &str) {
        let resolved = self.tabs.get(self.active).and_then(|tab| {
            tab.table
                .definitions()
                .iter()
                .find(|d| d.name == register_name)
                .map(|d| (d.register.clone(), tab.spec.role))
        });
        let Some((register, role)) = resolved else {
            self.log_active(format!(":set unknown register '{register_name}'"))
                .await;
            return;
        };
        // Virtual registers store their value in the module's virtual_values map (server only).
        if let Address::Virtual = register.address() {
            if role == Role::Server {
                self.tabs[self.active]
                    .module
                    .set_virtual_value(register_name, crate::module::str_to_value(value, &register))
                    .await;
                self.log_active(format!("set {register_name} = {value} (virtual)"))
                    .await;
            } else {
                self.log_active(format!(
                    ":set '{register_name}' is virtual — only writable on servers"
                ))
                .await;
            }
            return;
        }
        let addr = match register.address() {
            Address::Fixed(a) => *a,
            Address::Virtual => unreachable!(),
        };
        let raw = match register.encode(value) {
            Ok(raw) => raw,
            Err(e) => {
                self.log_active(format!(":set encode error: {e}")).await;
                return;
            }
        };
        let slave = *register.slave_id();

        match role {
            Role::Server => {
                let memory = {
                    let tab = &self.tabs[self.active];
                    tab.module.memory()
                };
                let ok = {
                    let key = Key {
                        id: SlaveKey {
                            slave_id: slave,
                            kind: register.kind().clone(),
                        },
                    };
                    let range = Range::new(addr as usize, raw.len());
                    let mut guard = memory.write().await;
                    // Read-modify-write: merge the masked field into the existing word so a
                    // sibling register aliasing this address keeps its bits.
                    let old = guard
                        .read_unchecked(key.clone(), &range)
                        .unwrap_or_default();
                    let merged = register.merge_write(&old, &raw);
                    guard.write_unchecked(key, &range, &merged)
                };
                if ok {
                    self.log_active(format!("set {register_name} = {value}"))
                        .await;
                } else {
                    self.log_active(format!(
                        ":set '{register_name}' rejected (addr {addr}, slave {slave}, {raw:?} not writable)"
                    ))
                    .await;
                }
            }
            Role::Client => {
                let key = Key {
                    id: SlaveKey {
                        slave_id: slave,
                        kind: register.kind().clone(),
                    },
                };
                let range = Range::new(addr as usize, raw.len());
                // Read-modify-write against the local mirror (last polled value) so a masked
                // write preserves the bits of any sibling register sharing this address. An
                // unpolled address mirrors zeros, leaving the out-of-mask bits 0.
                let merged = {
                    let memory = self.tabs[self.active].module.memory();
                    let old = memory
                        .read()
                        .await
                        .read_unchecked(key.clone(), &range)
                        .unwrap_or_default();
                    register.merge_write(&old, &raw)
                };
                let command = write_command(&register, slave, addr, &merged);
                let result = self.tabs[self.active].module.send_command(command).await;
                match result {
                    Ok(()) => {
                        // Write-only registers are never polled back, so mirror the value into
                        // local memory immediately so the table reflects what was sent.
                        if *register.access() == Access::WriteOnly {
                            let memory = self.tabs[self.active].module.memory();
                            memory.write().await.write_unchecked(key, &range, &merged);
                        }
                        self.log_active(format!("set {register_name} = {value} (sent)"))
                            .await
                    }
                    Err(e) => self.log_active(format!(":set failed: {e}")).await,
                }
            }
        }
    }

    /// Save the active tab's device configuration to a file.
    fn save_device(&self, path: &str) -> Result<(), String> {
        let ty = FileType::from_path(path)
            .ok_or_else(|| format!("unknown format for '{path}' (use .toml or .json)"))?;
        let tab = self.tabs.get(self.active).ok_or("no active tab")?;
        let mut device = tab.device.clone();
        device.version = Some(crate::config::VERSION.to_string());
        Converter::save(&device, path, ty).map_err(|e| format!("{e:?}"))
    }

    /// Save the current module instances as a session file.
    fn save_session(&self, path: &str) -> Result<(), String> {
        let ty = FileType::from_path(path)
            .ok_or_else(|| format!("unknown format for '{path}' (use .toml or .json)"))?;
        let session = Session {
            version: Some(crate::config::VERSION.to_string()),
            modules: self.tabs.iter().map(|t| t.spec.clone()).collect(),
        };
        Converter::save(&session, path, ty).map_err(|e| format!("{e:?}"))
    }
}
