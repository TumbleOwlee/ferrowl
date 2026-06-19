//! Execution of `:` commands against the active tab: module lifecycle, value writes,
//! ordering and persistence.

use ferrowl_codec::{Access, Address};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::Range;
use ferrowl_util::convert::{Converter, FileType};

use crate::config::{Role, Session};
use crate::module::Module;
use crate::module::view::CommandResult;

use crate::module::modbus::registers::write_command;

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
                let _ = self.tabs[self.active].modbus_mut().module_mut().stop().await;
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
                let raw = match action {
                    LuaCommand::Start => "lua start",
                    LuaCommand::Stop => "lua stop",
                    LuaCommand::Status => "lua status",
                };
                let result = self.tabs.get_mut(self.active)
                    .map(|tab| tab.view.handle_command(raw))
                    .unwrap_or(CommandResult::Unhandled);
                if let CommandResult::Handled(Some(msg)) = result {
                    self.log_active(msg).await;
                }
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
                        Some(t) if !t.modbus().spec.device.is_empty() => &t.modbus().spec.device,
                        Some(t) if t.modbus().spec.device.is_empty() => {
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
                            tab.log.write().await.clear();
                        }
                    } else if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.modbus_mut().device.log_file = Some(file.clone());
                        tab.modbus_mut().module_mut().set_log_base(Some(&file));
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
                let raw = match column.as_deref() {
                    None => "order".to_string(),
                    Some(col) => format!("order {} {}", col, if descending { "desc" } else { "asc" }),
                };
                let result = self.tabs.get_mut(self.active)
                    .map(|tab| tab.view.handle_command(&raw))
                    .unwrap_or(CommandResult::Unhandled);
                if let CommandResult::Handled(Some(msg)) = result {
                    self.log_active(msg).await;
                }
            }
            Cmd::Unknown(name) => {
                let result = self.tabs.get_mut(self.active)
                    .map(|tab| tab.view.handle_command(input))
                    .unwrap_or(CommandResult::Unhandled);
                match result {
                    CommandResult::Handled(msg) => {
                        if let Some(m) = msg { self.log_active(m).await; }
                    }
                    CommandResult::Unhandled => {
                        self.log_active(format!("Unknown command ':{name}'")).await;
                    }
                }
            }
        }
        false
    }

    async fn reload_module(&mut self) {
        let active = self.active;
        let Some(tab) = self.tabs.get(active) else {
            return;
        };
        if tab.modbus().spec.device.is_empty() {
            self.log_active("No configuration file path configured. Reload aborted.".to_string())
                .await;
            return;
        }
        let path = tab.modbus().spec.device.clone();
        let device = match crate::config::load_device(&path) {
            Ok(d) => d,
            Err(e) => {
                self.log_active(format!(":reload failed to load '{path}': {e}"))
                    .await;
                return;
            }
        };
        let _ = self.tabs[active].modbus_mut().module_mut().stop().await;
        let spec = self.tabs[active].modbus().spec.clone();
        let mut module = Module::new(&spec, &device);
        if let Err(e) = module.start().await {
            self.log_active(format!(":reload start error: {e}")).await;
        }
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
        let role = self.tabs[active].modbus().spec.role.to_string();
        let result = self.tabs[active].modbus_mut().module_mut().start().await;
        let msg = match result {
            Ok(()) => format!("Started {role} on {}", self.tabs[active].modbus().spec.endpoint),
            Err(e) => format!("Start {role} failed: {e}"),
        };
        self.tabs[active].log.write().await.write(&msg);
    }

    async fn stop_module(&mut self) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        let role = self.tabs[active].modbus().spec.role.to_string();
        let result = self.tabs[active].modbus_mut().module_mut().stop().await;
        let msg = match result {
            Ok(()) => format!("Stopped {role}"),
            Err(e) => format!("Stop {role} failed: {e}"),
        };
        self.tabs[active].log.write().await.write(&msg);
    }

    /// Write a value to a register on the active module: local memory for servers, a modbus
    /// write command for clients.
    pub(super) async fn set_value(&mut self, register_name: &str, value: &str) {
        let resolved = self.tabs.get(self.active).and_then(|tab| {
            tab.modbus()
                .table()
                .definitions()
                .iter()
                .find(|d| d.name == register_name)
                .map(|d| (d.register.clone(), tab.modbus().spec.role))
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
                    .modbus_mut()
                    .module_mut()
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
                    tab.modbus().module().memory()
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
                    let memory = self.tabs[self.active].modbus().module().memory();
                    let old = memory
                        .read()
                        .await
                        .read_unchecked(key.clone(), &range)
                        .unwrap_or_default();
                    register.merge_write(&old, &raw)
                };
                let command = write_command(&register, slave, addr, &merged);
                let result = self.tabs[self.active].modbus_mut().module_mut().send_command(command).await;
                match result {
                    Ok(()) => {
                        // Write-only registers are never polled back, so mirror the value into
                        // local memory immediately so the table reflects what was sent.
                        if *register.access() == Access::WriteOnly {
                            let memory = self.tabs[self.active].modbus().module().memory();
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
        let mut device = tab.modbus().device.clone();
        device.version = Some(crate::config::VERSION.to_string());
        Converter::save(&device, path, ty).map_err(|e| format!("{e:?}"))
    }

    /// Save the current module instances as a session file.
    fn save_session(&self, path: &str) -> Result<(), String> {
        let ty = FileType::from_path(path)
            .ok_or_else(|| format!("unknown format for '{path}' (use .toml or .json)"))?;
        let session = Session {
            version: Some(crate::config::VERSION.to_string()),
            modules: self.tabs.iter().map(|t| t.modbus().spec.clone()).collect(),
        };
        Converter::save(&session, path, ty).map_err(|e| format!("{e:?}"))
    }
}
