//! Overlay (modal dialog) lifecycle: opening the setup/edit/add dialogs, routing keys into
//! them, and applying their results to the active tab.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_codec::Address;

use crate::config::device::RegisterDef;
use crate::dialog::{EditedRegister, SetupDialog, SetupValues};
use crate::module::modbus::registers::{collect_scripts, register_mem_binding, sync_register_def};
use crate::module::modbus::setup::ModbusSetupView;
use crate::module::modbus::table::Definition;
use crate::module::view::ModuleView;

use super::{App, Focus, Overlay, OverlayAction, Tab};

impl App {
    pub(super) async fn handle_dialog_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> bool {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => self.close_overlay(),
            (KeyModifiers::NONE, KeyCode::Enter) => self.confirm_overlay().await,
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                if let Some(o) = self.overlay.as_mut() {
                    o.focus_previous();
                }
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if let Some(o) = self.overlay.as_mut() {
                    o.focus_next();
                }
            }
            _ => {
                if let Some(o) = self.overlay.as_mut() {
                    o.handle_events(modifiers, code);
                }
            }
        }
        false
    }

    /// Confirm the active overlay. Applies only when the dialog fully validates; otherwise it
    /// stays open (Esc cancels). The action is computed before mutating `self`.
    async fn confirm_overlay(&mut self) {
        let action = match &self.overlay {
            Some(Overlay::Creation(sv)) => sv.confirm().map(|(name, factory)| {
                OverlayAction::CreateTab { name, view: factory() }
            }),
            Some(Overlay::Setup(d)) => d.resolve().ok().map(|o| OverlayAction::ApplySetup(o.values)),
            None => None,
        };
        let Some(action) = action else {
            return;
        };

        match action {
            OverlayAction::CreateTab { name, view } => self.create_tab(name, view).await,
            OverlayAction::ApplySetup(values) => self.apply_setup(values).await,
        }
        self.close_overlay();
    }

    /// Open the setup dialog pre-filled from the active tab's instance settings (`:e`).
    pub(super) fn enter_setup(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let mv = tab.modbus();
        let timing = crate::module::Module::resolve_timing(&mv.spec, &mv.device);
        let dialog = SetupDialog::edit(
            &mv.spec.name,
            &mv.spec.device,
            mv.spec.role,
            &mv.spec.endpoint,
            (timing.timeout_ms, timing.delay_ms, timing.interval_ms),
            &mv.device.read_ranges,
        );
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the creation dialog for a new module tab (`:n`/`:new`).
    pub(super) fn enter_new(&mut self) {
        self.overlay = Some(Overlay::Creation(Box::new(ModbusSetupView::new_create())));
        self.focus = Focus::Dialog;
    }

    /// Open the creation dialog pre-filled with an optional device-config path (`:l`).
    pub(super) fn enter_load(&mut self, path: Option<&str>) {
        let mut sv = ModbusSetupView::new_create();
        if let Some(path) = path {
            sv.dialog_mut().config_path.state.set_input(path.to_string());
            sv.dialog_mut().config_path.state.set_cursor(path.chars().count());
        }
        self.overlay = Some(Overlay::Creation(Box::new(sv)));
        self.focus = Focus::Dialog;
    }

    /// Open the edit dialog for the selected register row (Enter in the table).
    pub(super) fn open_edit(&mut self) {
        if self.focus != Focus::Table {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.modbus_mut().open_edit();
        }
    }

    /// Open a blank add-register dialog (`:add`).
    pub(super) fn enter_add(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.modbus_mut().open_add();
        }
    }

    /// Insert a newly-created register (from the `:add` dialog) into the active tab.
    pub(super) async fn apply_add(&mut self, edited: EditedRegister) {
        let active = self.active;
        let Some(tab) = self.tabs.get_mut(active) else {
            return;
        };

        let named_values = edited.named_values.clone().unwrap_or_default();

        // Build a RegisterDef from the edited register; reuse sync_register_def for format fields.
        let mut def = RegisterDef {
            slave_id: 0,
            read_code: 4,
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
            update: edited.update.as_ref().filter(|s| !s.is_empty()).cloned(),
            description: edited.description.clone(),
            default: edited.default.clone(),
        };
        sync_register_def(&mut def, &edited.register);

        tab.modbus_mut().device.definitions.insert(edited.name.clone(), def);
        tab.modbus_mut().module_mut().add_register(
            edited.name.clone(),
            edited.description.clone(),
            edited.register.clone(),
            named_values.clone(),
        );

        if let Some((kind, key, range)) = register_mem_binding(&edited.register) {
            tab.modbus_mut()
                .module_mut()
                .memory()
                .write()
                .await
                .add_ranges(key, &kind, &[range]);
        }

        if let Some(tab) = self.tabs.get(active) {
            tab.modbus().module().rebuild_operations().await;
        }

        // Update table view.
        if let Some(tab) = self.tabs.get_mut(active) {
            let mut defs = { tab.modbus().table().definitions().to_vec() };
            defs.push(Definition::new(
                edited.name.clone(),
                edited.description.clone(),
                edited.register.clone(),
                named_values,
            ));
            tab.modbus_mut().table_mut().set_definitions(defs);
        }

        // Reload Lua sim when the new register has an update script.
        if edited
            .update
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            && let Some(tab) = self.tabs.get_mut(active)
        {
            let scripts = collect_scripts(&tab.modbus().device);
            tab.modbus_mut().module_mut().reload_scripts(scripts);
        }

        // Seed a virtual register with a zero/empty value so it shows up before a script or
        // `:set` runs. The configured default or explicit value below will override this.
        if let Address::Virtual = edited.register.address() {
            let seed = crate::module::default_value(&edited.register);
            if let Some(tab) = self.tabs.get(active) {
                tab.modbus().module().set_virtual_value(&edited.name, seed).await;
            }
        }

        // Apply configured default as the initial value when no explicit value was given.
        if edited.value.is_none()
            && let Some(ref default_scalar) = edited.default
        {
            self.set_value(&edited.name, &default_scalar.to_string())
                .await;
        }

        if let Some(value) = edited.value {
            self.set_value(&edited.name, &value).await;
        }
    }

    /// Apply edited register metadata to the selected row, then optionally write its value.
    pub(super) async fn apply_edit(&mut self, edited: EditedRegister) {
        use crate::config::session::Role;
        let active = self.active;
        let mut preserved_value: Option<String> = None;
        let mem_update = if let Some(tab) = self.tabs.get_mut(active)
            && let Some(idx) = tab.modbus().table().selected_index()
        {
            let mut defs = tab.modbus().table().definitions().to_vec();
            let update = if let Some(slot) = defs.get_mut(idx) {
                let original_name = slot.name.clone();
                let named_values = edited
                    .named_values
                    .clone()
                    .unwrap_or_else(|| slot.named_values.clone());

                // Issue 8: preserve current value on servers when the format changes but address
                // stays the same and the user left the value field blank.
                if tab.modbus().spec.role == Role::Server
                    && edited.value.is_none()
                    && slot.register.address() == edited.register.address()
                    && !slot.value.is_empty()
                {
                    // Unscaled string: `set_value` re-encodes it, and `encode` takes raw values.
                    preserved_value = Some(slot.value.clone().unscaled().to_string());
                }

                // Issue 9: keep module's register cache in sync so rebuild_operations is correct.
                tab.modbus_mut().module_mut().update_register(
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
                    .map(|(kind, key, range)| (tab.modbus().module().memory(), key, kind, range));

                // Issue 11: look up by original name, update description, handle rename.
                if let Some(def) = tab.modbus_mut().device.definitions.get_mut(&original_name) {
                    sync_register_def(def, &edited.register);
                    def.description = edited.description.clone();
                    if let Some(nv) = &edited.named_values {
                        def.values = nv.clone();
                    }
                    if let Some(script) = &edited.update {
                        def.update = if script.is_empty() {
                            None
                        } else {
                            Some(script.clone())
                        };
                    }
                    def.default = edited.default.clone();
                }
                if edited.name != original_name
                    && let Some(def) = tab.modbus_mut().device.definitions.remove(&original_name)
                {
                    tab.modbus_mut().device.definitions.insert(edited.name.clone(), def);
                }

                mem_result
            } else {
                None
            };
            tab.modbus_mut().table_mut().set_definitions(defs);
            update
        } else {
            None
        };

        if let Some((memory, key, kind, range)) = mem_update {
            memory.write().await.add_ranges(key, &kind, &[range]);
        }

        // Issue 9: refresh client operations after register metadata changed.
        if let Some(tab) = self.tabs.get(active) {
            tab.modbus().module().rebuild_operations().await;
        }

        // Reload the Lua sim thread when the edit included a script change so the new script
        // takes effect immediately rather than only on the next module start.
        if edited.update.is_some()
            && let Some(tab) = self.tabs.get_mut(active)
        {
            let scripts = collect_scripts(&tab.modbus().device);
            tab.modbus_mut().module_mut().reload_scripts(scripts);
        }

        // Issue 8: re-apply the old value when only the format changed.
        if let Some(v) = preserved_value
            && edited.value.is_none()
        {
            self.set_value(&edited.name, &v).await;
        }

        if let Some(value) = edited.value {
            self.set_value(&edited.name, &value).await;
        }
    }

    /// Apply confirmed setup values to the active tab. Rebuilds and restarts the module's
    /// instance so role/endpoint changes (e.g. client↔server) take effect immediately and the
    /// spec stays in sync with the running instance.
    async fn apply_setup(&mut self, values: SetupValues) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        self.tabs[active].name = values.name.clone();
        {
            let mv = self.tabs[active].modbus_mut();
            mv.spec.device = values.config_path.clone();
            mv.spec.name = values.name;
            mv.spec.role = values.role;
            mv.spec.endpoint = values.endpoint.clone();
            mv.spec.timeout_ms = values.timeout_ms;
            mv.spec.delay_ms = values.delay_ms;
            mv.spec.interval_ms = values.interval_ms;
            mv.device.timeout_ms = values.timeout_ms;
            mv.device.delay_ms = values.delay_ms;
            mv.device.interval_ms = values.interval_ms;
            mv.device.read_ranges = values.read_ranges.clone();
        }

        let timing = {
            let mv = self.tabs[active].modbus();
            crate::module::Module::resolve_timing(&mv.spec, &mv.device)
        };
        if let Err(e) = self.tabs[active]
            .modbus_mut()
            .module_mut()
            .reconfigure(&values.endpoint, values.role, timing, values.read_ranges)
            .await
        {
            self.tabs[active]
                .log
                .write()
                .await
                .write(&format!("Reconfigure failed: {e}"));
            return;
        }
        self.start_module().await;
    }

    /// Create and append a new tab from a `Box<dyn ModuleView>`, then start its module.
    async fn create_tab(&mut self, name: String, view: Box<dyn ModuleView>) {
        self.tabs.push(Tab::new_from_view(name, view));
        self.active = self.tabs.len() - 1;
        self.start_module().await;
    }

    /// Delete a named register from the active tab's device config, module, and table view.
    pub(super) async fn delete_register_by_name(&mut self, name: String) {
        let active = self.active;

        if let Some(tab) = self.tabs.get_mut(active) {
            tab.modbus_mut().device.definitions.remove(&name);
            tab.modbus_mut().module_mut().remove_register_by_name(&name);
            let mut defs = { tab.modbus().table().definitions().to_vec() };
            defs.retain(|d| d.name != name);
            tab.modbus_mut().table_mut().set_definitions(defs);
            tab.modbus_mut().table_mut().select_first();
        }

        if let Some(tab) = self.tabs.get(active) {
            tab.modbus().module().rebuild_operations().await;
        }

        if let Some(tab) = self.tabs.get_mut(active) {
            let scripts = collect_scripts(&tab.modbus().device);
            tab.modbus_mut().module_mut().reload_scripts(scripts);
        }
    }
}
