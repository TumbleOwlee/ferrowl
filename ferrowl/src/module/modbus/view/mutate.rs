//! Applying confirmed dialog/setup results to the module: register add/edit/delete, setup
//! reconfiguration, and writing register values.

use ferrowl_codec::{Access, Address, Kind};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::Range;
use ferrowl_ui::widgets::Header;

use crate::app::Level;
use crate::module::modbus::dialog::EditedRegister;
use crate::module::modbus::setup_dialog::SetupValues;
use crate::module::modbus::table::{Definition, TableHeader, column_index};
use crate::module::view::CommandResult;

use super::super::ModbusModule;
use super::super::registers::{register_mem_binding, sync_register_def, write_command};
use super::ModbusModuleView;

/// Classifies a `:set`-style status message for the log ring: outright failures are `Error`,
/// rejected/invalid input is `Warning`, and successful sets are `Info`.
fn set_result_level(msg: &str) -> Level {
    let lower = msg.to_lowercase();
    if lower.contains("failed") || lower.contains("error") {
        Level::Error
    } else if msg.starts_with(':') {
        Level::Warning
    } else {
        Level::Info
    }
}

impl ModbusModuleView {
    pub(super) fn apply_order(&mut self, col: &str, descending: bool) -> CommandResult {
        match column_index(col) {
            None => CommandResult::Handled(Some(format!("Unknown column '{col}'"))),
            Some(idx) => {
                self.sort = Some((idx, descending));
                self.table.sort_definitions(idx, descending);
                let header = TableHeader::header()[idx].clone();
                let dir = if descending { "DESC" } else { "ASC" };
                CommandResult::Handled(Some(format!("Ordered by {header} {dir}")))
            }
        }
    }

    pub(super) fn save_device_to(&self, path: &str) -> CommandResult {
        use ferrowl_util::convert::{Converter, FileType};
        let Some(ty) = FileType::from_path(path) else {
            return CommandResult::Handled(Some(format!(
                "unknown format for '{path}' (use .toml or .json)"
            )));
        };
        let mut device = self.device.clone();
        device.version = Some(crate::config::VERSION.to_string());
        match Converter::save(&device, path, ty) {
            Ok(()) => CommandResult::Handled(Some(format!("Saved device config to {path}"))),
            Err(e) => CommandResult::Handled(Some(format!("Save failed: {e:?}"))),
        }
    }

    pub(super) async fn apply_add(&mut self, edited: EditedRegister) {
        let named_values = edited.named_values.clone().unwrap_or_default();

        let mut def = crate::config::device::RegisterDef {
            slave_id: 0,
            kind: Kind::HoldingRegister,
            address: None,
            is_virtual: false,
            access: crate::config::device::AccessCfg::ReadWrite,
            value_type: crate::config::device::ValueType::U16,
            endian: crate::config::device::EndianCfg::default(),
            resolution: 1.0,
            bitmask: None,
            length: 1,
            alignment: crate::config::device::AlignmentCfg::default(),
            values: named_values.clone(),
            update: None,
            description: edited.description.clone(),
            default: edited.default.clone(),
        };
        sync_register_def(&mut def, &edited.register);

        self.device.definitions.insert(edited.name.clone(), def);
        self.module.add_register(
            edited.name.clone(),
            edited.description.clone(),
            edited.register.clone(),
            named_values.clone(),
        );

        if let Some((kind, key, range)) = register_mem_binding(&edited.register) {
            self.module
                .memory()
                .write()
                .add_ranges(key, &kind, &[range]);
        }

        self.module.rebuild_operations().await;

        let mut defs = self.table.definitions().to_vec();
        defs.push(Definition::new(
            edited.name.clone(),
            edited.description.clone(),
            edited.register.clone(),
            named_values,
        ));
        self.table.set_definitions(defs);

        if let Address::Virtual = edited.register.address() {
            let seed = crate::module::modbus::default_value(&edited.register);
            self.module.set_virtual_value(&edited.name, seed).await;
        }

        if edited.value.is_none()
            && let Some(ref default_scalar) = edited.default
        {
            let result = self
                .set_register_value(&edited.name, &default_scalar.to_string())
                .await;
            if let CommandResult::Handled(Some(msg)) = result {
                self.module
                    .log()
                    .write()
                    .await
                    .write(set_result_level(&msg), &msg);
            }
        }

        if let Some(value) = edited.value {
            let result = self.set_register_value(&edited.name, &value).await;
            if let CommandResult::Handled(Some(msg)) = result {
                self.module
                    .log()
                    .write()
                    .await
                    .write(set_result_level(&msg), &msg);
            }
        }
    }

    pub(super) async fn apply_edit(
        &mut self,
        edited: EditedRegister,
        idx: usize,
        original_name: String,
    ) {
        use crate::config::session::Role;

        let mut preserved_value: Option<String> = None;
        let mut defs = self.table.definitions().to_vec();

        let mem_update = if let Some(slot) = defs.get_mut(idx) {
            let named_values = edited
                .named_values
                .clone()
                .unwrap_or_else(|| slot.named_values.clone());

            if self.spec.role == Role::Server
                && edited.value.is_none()
                && slot.register.address() == edited.register.address()
                && !slot.value.is_empty()
            {
                preserved_value = Some(slot.value.clone().unscaled().to_string());
            }

            self.module.update_register(
                idx,
                edited.name.clone(),
                edited.description.clone(),
                edited.register.clone(),
                named_values.clone(),
            );

            *slot = Definition::new(
                edited.name.clone(),
                edited.description.clone(),
                edited.register.clone(),
                named_values,
            );

            let mem_result = register_mem_binding(&edited.register)
                .map(|(kind, key, range)| (self.module.memory(), key, kind, range));

            if let Some(def) = self.device.definitions.get_mut(&original_name) {
                sync_register_def(def, &edited.register);
                def.description = edited.description.clone();
                if let Some(nv) = &edited.named_values {
                    def.values = nv.clone();
                }
                def.default = edited.default.clone();
            }
            if edited.name != original_name
                && let Some(def) = self.device.definitions.remove(&original_name)
            {
                self.device.definitions.insert(edited.name.clone(), def);
            }

            mem_result
        } else {
            None
        };

        self.table.set_definitions(defs);

        if let Some((memory, key, kind, range)) = mem_update {
            memory.write().add_ranges(key, &kind, &[range]);
        }

        self.module.rebuild_operations().await;

        if let Some(v) = preserved_value
            && edited.value.is_none()
        {
            let result = self.set_register_value(&edited.name, &v).await;
            if let CommandResult::Handled(Some(msg)) = result {
                self.module
                    .log()
                    .write()
                    .await
                    .write(set_result_level(&msg), &msg);
            }
        }

        if let Some(value) = edited.value {
            let result = self.set_register_value(&edited.name, &value).await;
            if let CommandResult::Handled(Some(msg)) = result {
                self.module
                    .log()
                    .write()
                    .await
                    .write(set_result_level(&msg), &msg);
            }
        }
    }

    pub(super) async fn delete_register_by_name(&mut self, name: String) {
        self.device.definitions.remove(&name);
        self.module.remove_register_by_name(&name);
        let mut defs = self.table.definitions().to_vec();
        defs.retain(|d| d.name != name);
        self.table.set_definitions(defs);
        self.table.select_first();

        self.module.rebuild_operations().await;
    }

    pub(super) async fn apply_setup(&mut self, values: SetupValues) {
        self.spec.device = values.config_path.clone();
        self.spec.name = values.name.clone();
        self.spec.role = values.role;
        self.spec.endpoint = values.endpoint.clone();
        self.device.timeout_ms = values.timeout_ms;
        self.device.delay_ms = values.delay_ms;
        self.device.interval_ms = values.interval_ms;
        if let Some(reconnect) = values.reconnect {
            self.device.reconnect = Some(reconnect);
        }
        self.device.read_ranges = values.read_ranges.clone();

        let timing = ModbusModule::resolve_timing(&self.device);
        let role = self.spec.role.to_string();
        let endpoint = self.spec.endpoint.to_string();

        if let Err(e) = self
            .module
            .reconfigure(&values.endpoint, values.role, timing, values.read_ranges)
            .await
        {
            self.module
                .log()
                .write()
                .await
                .write(Level::Error, &format!("Reconfigure failed: {e}"));
            return;
        }
        match self.module.start().await {
            Ok(()) => {
                self.module
                    .log()
                    .write()
                    .await
                    .write(Level::Info, &format!("Started {role} on {endpoint}"));
            }
            Err(e) => {
                self.module
                    .log()
                    .write()
                    .await
                    .write(Level::Error, &format!("Start {role} failed: {e}"));
            }
        }
    }

    pub(super) async fn set_register_value(
        &mut self,
        register_name: &str,
        value: &str,
    ) -> CommandResult {
        use crate::config::Role;

        let resolved = self
            .table
            .definitions()
            .iter()
            .find(|d| d.name == register_name)
            .map(|d| (d.register.clone(), self.spec.role));

        let Some((register, role)) = resolved else {
            return CommandResult::Handled(Some(format!(
                ":set unknown register '{register_name}'"
            )));
        };

        if let Address::Virtual = register.address() {
            if role == Role::Server {
                self.module
                    .set_virtual_value(
                        register_name,
                        crate::module::modbus::str_to_value(value, &register),
                    )
                    .await;
                return CommandResult::Handled(Some(format!(
                    "set {register_name} = {value} (virtual)"
                )));
            } else {
                return CommandResult::Handled(Some(format!(
                    ":set '{register_name}' is virtual — only writable on servers"
                )));
            }
        }

        let addr = match register.address() {
            Address::Fixed(a) => *a,
            Address::Virtual => unreachable!(),
        };
        let raw = match register.encode(value) {
            Ok(r) => r,
            Err(e) => return CommandResult::Handled(Some(format!(":set encode error: {e}"))),
        };
        let slave = *register.slave_id();

        match role {
            Role::Server => {
                let memory = self.module.memory();
                let key = Key {
                    id: SlaveKey {
                        slave_id: slave,
                        kind: register.kind().clone(),
                    },
                };
                let range = Range::new(addr as usize, raw.len());
                let ok = {
                    let mut guard = memory.write();
                    let old = guard
                        .read_unchecked(key.clone(), &range)
                        .unwrap_or_default();
                    let merged = register.merge_write(&old, &raw);
                    guard.write_unchecked(key, &range, &merged)
                };
                if ok {
                    CommandResult::Handled(Some(format!("set {register_name} = {value}")))
                } else {
                    CommandResult::Handled(Some(format!(
                        ":set '{register_name}' rejected (addr {addr}, slave {slave}, {raw:?} not writable)"
                    )))
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
                let merged = {
                    let memory = self.module.memory();
                    let old = memory
                        .read()
                        .read_unchecked(key.clone(), &range)
                        .unwrap_or_default();
                    register.merge_write(&old, &raw)
                };
                let command = write_command(&register, slave, addr, &merged);
                let result = self.module.send_command(command).await;
                match result {
                    Ok(()) => {
                        if *register.access() == Access::WriteOnly {
                            let memory = self.module.memory();
                            memory.write().write_unchecked(key, &range, &merged);
                        }
                        CommandResult::Handled(Some(format!(
                            "set {register_name} = {value} (sent)"
                        )))
                    }
                    Err(e) => CommandResult::Handled(Some(format!(":set failed: {e}"))),
                }
            }
        }
    }
}
