//! Applying confirmed dialog/setup results to the module: register add/edit/delete, setup
//! reconfiguration, and writing register values.

use ferrowl_codec::{Access, Address, Kind};
use ferrowl_modbus::{Address as WireAddress, Key, SlaveKey};
use ferrowl_store::Range;
use ferrowl_ui::widgets::Header;

use crate::app::Level;
use crate::module::modbus::dialog::EditedRegister;
use crate::module::modbus::setup_dialog::SetupValues;
use crate::module::modbus::table::{Definition, TableHeader, column_index};
use crate::module::view::CommandResult;

use super::super::ModbusModule;
use super::super::build::declare_or_reject_msg;
use super::super::registers::{register_mem_binding, sync_register_def, write_command};
use super::ModbusModuleView;

impl ModbusModuleView {
    pub(super) fn apply_order(&mut self, col: &str, descending: bool) -> CommandResult {
        match column_index(col) {
            None => {
                CommandResult::Handled(Some((Level::Warning, format!("Unknown column '{col}'"))))
            }
            Some(idx) => {
                self.sort = Some((idx, descending));
                self.table.sort_definitions(idx, descending);
                let header = TableHeader::header()[idx].clone();
                let dir = if descending { "DESC" } else { "ASC" };
                CommandResult::Handled(Some((Level::Info, format!("Ordered by {header} {dir}"))))
            }
        }
    }

    pub(super) fn save_device_to(&self, path: &str) -> CommandResult {
        use ferrowl_util::convert::{Converter, FileType};
        let Some(ty) = FileType::from_path(path) else {
            return CommandResult::Handled(Some((
                Level::Warning,
                format!("unknown format for '{path}' (use .toml or .json)"),
            )));
        };
        let mut device = self.device.clone();
        device.version = Some(crate::config::VERSION.to_string());
        match Converter::save(&device, path, ty) {
            Ok(()) => CommandResult::Handled(Some((
                Level::Info,
                format!("Saved device config to {path}"),
            ))),
            Err(e) => CommandResult::Handled(Some((Level::Error, format!("Save failed: {e:?}")))),
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
            word_order: crate::config::device::WordOrderCfg::default(),
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
            let subject = format!("register '{}'", edited.name);
            let rejected = {
                let memory = self.module.memory();
                let mut mem = memory.write();
                declare_or_reject_msg(&mut mem, key, &kind, &range, &subject).err()
            };
            if let Some(msg) = rejected {
                self.module.log().write().await.write(Level::Warning, &msg);
            }
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
            if let CommandResult::Handled(Some((level, msg))) = result {
                self.module.log().write().await.write(level, &msg);
            }
        }

        if let Some(value) = edited.value {
            let result = self.set_register_value(&edited.name, &value).await;
            if let CommandResult::Handled(Some((level, msg))) = result {
                self.module.log().write().await.write(level, &msg);
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
                def.description.clone_from(&edited.description);
                if let Some(nv) = &edited.named_values {
                    def.values.clone_from(nv);
                }
                def.default.clone_from(&edited.default);
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
            let subject = format!("register '{}'", edited.name);
            let rejected = {
                let mut mem = memory.write();
                declare_or_reject_msg(&mut mem, key, &kind, &range, &subject).err()
            };
            if let Some(msg) = rejected {
                self.module.log().write().await.write(Level::Warning, &msg);
            }
        }

        self.module.rebuild_operations().await;

        if let Some(v) = preserved_value
            && edited.value.is_none()
        {
            let result = self.set_register_value(&edited.name, &v).await;
            if let CommandResult::Handled(Some((level, msg))) = result {
                self.module.log().write().await.write(level, &msg);
            }
        }

        if let Some(value) = edited.value {
            let result = self.set_register_value(&edited.name, &value).await;
            if let CommandResult::Handled(Some((level, msg))) = result {
                self.module.log().write().await.write(level, &msg);
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
        self.spec.device.clone_from(&values.config_path);
        self.spec.name.clone_from(&values.name);
        self.spec.role = values.role;
        self.spec.endpoint = values.endpoint.clone();
        self.device.timeout_ms = values.timeout_ms;
        self.device.delay_ms = values.delay_ms;
        self.device.interval_ms = values.interval_ms;
        if let Some(reconnect) = values.reconnect {
            self.device.reconnect = Some(reconnect);
        }
        self.device.read_ranges = values.read_ranges.clone();
        if let Some(tls) = values.tls.clone() {
            self.device.tls = tls;
        }

        let timing = ModbusModule::resolve_timing(&self.device);
        let role = self.spec.role.to_string();
        let endpoint = self.spec.endpoint.to_string();

        if let Err(e) = self
            .module
            .reconfigure(
                &values.endpoint,
                values.role,
                timing,
                values.read_ranges,
                self.device.tls.clone(),
            )
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
        use crate::config::ClientOrServer;

        let resolved = self
            .table
            .definitions()
            .iter()
            .find(|d| d.name == register_name)
            .map(|d| (d.register.clone(), self.spec.role.client_or_server()));

        let Some((register, role)) = resolved else {
            return CommandResult::Handled(Some((
                Level::Warning,
                format!(":set unknown register '{register_name}'"),
            )));
        };

        if let Address::Virtual = register.address() {
            if role == ClientOrServer::Server {
                self.module
                    .set_virtual_value(
                        register_name,
                        crate::module::modbus::str_to_value(value, &register),
                    )
                    .await;
                return CommandResult::Handled(Some((
                    Level::Info,
                    format!("set {register_name} = {value} (virtual)"),
                )));
            } else {
                return CommandResult::Handled(Some((
                    Level::Warning,
                    format!(":set '{register_name}' is virtual — only writable on servers"),
                )));
            }
        }

        let addr = match register.address() {
            Address::Fixed(a) => *a,
            Address::Virtual => unreachable!(),
        };
        let raw = match register.encode(value) {
            Ok(r) => r,
            Err(e) => {
                return CommandResult::Handled(Some((
                    Level::Error,
                    format!(":set encode error: {e}"),
                )));
            }
        };
        let slave = *register.slave_id();

        match role {
            ClientOrServer::Server => {
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
                    CommandResult::Handled(Some((
                        Level::Info,
                        format!("set {register_name} = {value}"),
                    )))
                } else {
                    CommandResult::Handled(Some((
                        Level::Warning,
                        format!(
                            ":set '{register_name}' rejected (addr {addr}, slave {slave}, {raw:?} not writable)"
                        ),
                    )))
                }
            }
            ClientOrServer::Client => {
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
                if *register.access() != Access::ReadOnly {
                    let command = write_command(&register, slave, WireAddress(addr), &merged);
                    let result = self.module.send_command(command).await;
                    match result {
                        Ok(()) => {
                            if *register.access() == Access::WriteOnly {
                                let memory = self.module.memory();
                                memory.write().write_unchecked(key, &range, &merged);
                            }
                            CommandResult::Handled(Some((
                                Level::Info,
                                format!("set {register_name} = {value} (sent)"),
                            )))
                        }
                        Err(e) => CommandResult::Handled(Some((
                            Level::Error,
                            format!(":set failed: {e}"),
                        ))),
                    }
                } else {
                    CommandResult::Handled(None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeviceConfig, Endpoint, ModuleSpec, Role};
    use crate::module::modbus::dialog::EditedRegister;
    use ferrowl_codec::format::{BitField, Endian, Format, Resolution, WordOrder};
    use ferrowl_codec::{Access, Address, Kind, Register, RegisterBuilder};
    use ferrowl_modbus::UnitId;
    use ferrowl_test_support::reserve_temp_dir;

    fn spec(role: Role) -> ModuleSpec {
        ModuleSpec {
            name: "test".into(),
            device: String::new(),
            role,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
        }
    }

    fn view(role: Role) -> ModbusModuleView {
        let device = DeviceConfig::default();
        let spec = spec(role);
        let module = ModbusModule::new(&spec, &device);
        ModbusModuleView::new(module, spec, device)
    }

    fn holding(addr: u16) -> Register {
        RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(addr))
            .format(Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap()
    }

    fn holding_read_only(addr: u16) -> Register {
        RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadOnly)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(addr))
            .format(Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap()
    }

    fn virtual_reg() -> Register {
        RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Virtual)
            .format(Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap()
    }

    fn edited(name: &str, register: Register, value: Option<&str>) -> EditedRegister {
        EditedRegister {
            name: name.into(),
            description: "d".into(),
            register,
            value: value.map(str::to_string),
            named_values: None,
            default: None,
        }
    }

    fn msg(result: &CommandResult) -> String {
        match result {
            CommandResult::Handled(Some((_, m))) => m.clone(),
            _ => panic!("expected a Handled message with text"),
        }
    }

    #[test]
    fn ut_apply_order_sets_sort_and_warns_unknown() {
        let mut v = view(Role::Server);
        let ordered = v.apply_order("Address", false);
        assert!(v.sort.is_some());
        assert!(msg(&ordered).starts_with("Ordered by"));
        assert!(msg(&v.apply_order("Nonsense", false)).contains("Unknown column"));
    }

    #[tokio::test]
    /// MB-R-088 — adding a register at runtime inserts its definition and rebuilds the op list.
    async fn ut_apply_add_inserts_definition() {
        let mut v = view(Role::Server);
        v.apply_add(edited("hold", holding(0), None)).await;
        assert!(v.device.definitions.contains_key("hold"));
        assert!(v.table.definitions().iter().any(|d| d.name == "hold"));
    }

    #[tokio::test]
    /// MB-R-088 — editing a register updates and renames its definition.
    async fn ut_apply_edit_renames_definition() {
        let mut v = view(Role::Server);
        v.apply_add(edited("hold", holding(0), None)).await;
        v.apply_edit(edited("renamed", holding(0), None), 0, "hold".into())
            .await;
        assert!(v.device.definitions.contains_key("renamed"));
        assert!(!v.device.definitions.contains_key("hold"));
    }

    #[tokio::test]
    /// MB-R-088 — deleting a register removes its definition and rebuilds the op list.
    async fn ut_delete_register_removes_definition() {
        let mut v = view(Role::Server);
        v.apply_add(edited("hold", holding(0), None)).await;
        v.delete_register_by_name("hold".into()).await;
        assert!(!v.device.definitions.contains_key("hold"));
        assert!(v.table.definitions().is_empty());
    }

    #[tokio::test]
    /// MB-R-092 — a virtual-register write is accepted on a server and rejected on a client.
    async fn ut_set_virtual_value_server_accepts_client_rejects() {
        let mut server = view(Role::Server);
        server.apply_add(edited("v", virtual_reg(), None)).await;
        assert!(msg(&server.set_register_value("v", "42").await).contains("(virtual)"));

        let mut client = view(Role::Client);
        client.apply_add(edited("v", virtual_reg(), None)).await;
        assert!(
            msg(&client.set_register_value("v", "42").await).contains("only writable on servers")
        );
    }

    #[tokio::test]
    /// MB-R-091, MB-R-158 — a client write dispatches a Modbus write command (not a direct store write); the read-write register's store value is not updated by the `:set`.
    async fn ut_client_fixed_write_dispatches_command_not_store() {
        use ferrowl_modbus::{Key, SlaveKey};
        use ferrowl_store::Range;

        let mut client = view(Role::Client);
        client.apply_add(edited("hold", holding(0), None)).await;

        // The client path sends a Modbus write command; with no running instance the send fails,
        // and either way a read-write register's store is not written directly by `:set`.
        let result = client.set_register_value("hold", "5").await;
        assert!(msg(&result).contains("failed")); // took the client send-command branch, not the server memory-write branch

        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        let stored = client
            .module
            .memory()
            .read()
            .read_unchecked(key, &Range::new(0, 1));
        assert_ne!(stored, Some(vec![5]));
    }

    #[tokio::test]
    /// MB-R-159 — a `ReadOnly` fixed register write on a client is silently accepted: no
    /// `Command::Write*` reaches the command channel (proven the same way the neighbouring
    /// read-write test proves it *did* send: with no running instance the send branch always
    /// fails, so `Handled(None)` here rules out the send branch having run at all), and the
    /// store is left byte-identical.
    async fn ut_client_read_only_write_sends_no_command_and_touches_no_store() {
        let mut client = view(Role::Client);
        client
            .apply_add(edited("ro", holding_read_only(0), None))
            .await;

        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        let range = Range::new(0, 1);
        let before = client
            .module
            .memory()
            .read()
            .read_unchecked(key.clone(), &range);

        let result = client.set_register_value("ro", "5").await;
        assert!(
            matches!(result, CommandResult::Handled(None)),
            "a ReadOnly client write is silently accepted: no message, error or otherwise"
        );

        let after = client.module.memory().read().read_unchecked(key, &range);
        assert_eq!(
            after, before,
            "a ReadOnly client write must leave the store byte-identical"
        );
    }

    #[tokio::test]
    async fn ut_set_register_value_unknown_warns() {
        let mut v = view(Role::Server);
        assert!(msg(&v.set_register_value("nope", "1").await).contains("unknown register"));
    }

    #[tokio::test]
    async fn ut_set_fixed_value_on_server_writes_memory() {
        let mut v = view(Role::Server);
        v.apply_add(edited("hold", holding(0), None)).await;
        assert!(msg(&v.set_register_value("hold", "7").await).contains("set hold = 7"));
    }

    #[test]
    /// CS-R-001 — every configuration file shall be TOML or JSON; there is no YAML support.
    fn ut_save_device_to_rejects_unknown_extension() {
        let v = view(Role::Server);
        assert!(msg(&v.save_device_to("/tmp/x.yaml")).contains("unknown format"));
    }

    #[test]
    fn ut_save_device_to_writes_toml() {
        let v = view(Role::Server);
        let dir = reserve_temp_dir("ferrowl_modbus_mutate");
        let path = dir.join("mutate.toml");
        let p = path.to_str().unwrap();
        assert!(msg(&v.save_device_to(p)).contains("Saved device config"));
        assert!(path.exists());
    }

    fn device_with_gap() -> DeviceConfig {
        use crate::config::device::{
            AccessCfg, AlignmentCfg, EndianCfg, ReadRanges, RegisterDef, ValueType, WordOrderCfg,
        };
        use std::collections::BTreeMap;

        let mut definitions = BTreeMap::new();
        // Fixed holding register at address 8: leaves [0,8) and [9,10) as gaps once `read_ranges`
        // below spans [0,10) — see `explicit_read_coverage`, `build.rs`.
        definitions.insert(
            "hold".into(),
            RegisterDef {
                slave_id: 1,
                kind: Kind::HoldingRegister,
                address: Some(8),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::U16,
                endian: EndianCfg::Big,
                word_order: WordOrderCfg::default(),
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: AlignmentCfg::Left,
                values: vec![],
                update: None,
                description: "d".into(),
                default: None,
            },
        );
        DeviceConfig {
            definitions,
            read_ranges: ReadRanges {
                holding: Some("0-10".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    /// MB-R-129 — a runtime-added register colliding with a `read_ranges` gap cell (the reachable
    /// case in edge-cases.md §6.8) logs a Warning naming the register and the rejected range, from
    /// `apply_add`'s call site — `add_ranges`'s own silent rejection (no backing memory) is
    /// unaffected.
    async fn ut_apply_add_warns_on_gap_collision() {
        let device = device_with_gap();
        let spec = spec(Role::Server);
        let module = ModbusModule::new(&spec, &device);
        let mut v = ModbusModuleView::new(module, spec, device);

        // Address 2 falls inside the [0,8) gap, already declared Read; `holding(2)` is ReadWrite.
        v.apply_add(edited("new", holding(2), None)).await;

        let warned = v
            .module
            .log()
            .write()
            .await
            .peek_n(crate::app::LOG_SIZE)
            .iter()
            .any(|(_, level, msg)| {
                *level == Level::Warning && msg.contains("register 'new'") && msg.contains("[2, 3)")
            });
        assert!(
            warned,
            "expected a Warning naming the rejected register/range"
        );
    }

    #[tokio::test]
    /// MB-R-129 — editing a register into a `read_ranges` gap cell it collides with logs the same
    /// Warning, from `apply_edit`'s call site.
    async fn ut_apply_edit_warns_on_gap_collision() {
        let device = device_with_gap();
        let spec = spec(Role::Server);
        let module = ModbusModule::new(&spec, &device);
        let mut v = ModbusModuleView::new(module, spec, device);

        // Add harmlessly outside any gap, then edit its address into the gap.
        v.apply_add(edited("movable", holding(20), None)).await;
        v.apply_edit(edited("movable", holding(2), None), 1, "movable".into())
            .await;

        let warned = v
            .module
            .log()
            .write()
            .await
            .peek_n(crate::app::LOG_SIZE)
            .iter()
            .any(|(_, level, msg)| {
                *level == Level::Warning
                    && msg.contains("register 'movable'")
                    && msg.contains("[2, 3)")
            });
        assert!(
            warned,
            "expected a Warning naming the rejected register/range"
        );
    }
}
