//! Overlay (modal dialog) lifecycle: opening the setup/edit/add dialogs, routing keys into
//! them, and applying their results to the active tab.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_reg::Address;

use crate::config::{
    ModuleSpec,
    device::{DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_TIMEOUT_MS, RegisterDef},
};
use crate::dialog::{
    EditInputDialog, EditSelectionDialog, EditedRegister, SetupDialog, SetupValues, SubDialogs,
};
use crate::module::Module;
use crate::view::main::Definition;

use super::registers::{collect_scripts, register_mem_binding, sync_register_def};
use super::{App, Focus, Overlay, OverlayAction, Tab};

impl App {
    pub(super) async fn handle_dialog_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> bool {
        // When either edit dialog has an open add sub-dialog, route all keys into it.
        // Route keys to the delete-confirmation box while it is open; it takes priority over the
        // underlying edit dialog.
        let has_confirm_delete = matches!(&self.overlay,
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) if d.has_confirm_delete())
            || matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.has_confirm_delete());
        if has_confirm_delete {
            match code {
                KeyCode::Esc => match self.overlay.as_mut() {
                    Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => d.close_confirm_delete(),
                    Some(Overlay::EditSelection(d)) => d.close_confirm_delete(),
                    _ => {}
                },
                KeyCode::Tab => match self.overlay.as_mut() {
                    Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => d.confirm_delete_focus_next(),
                    Some(Overlay::EditSelection(d)) => d.confirm_delete_focus_next(),
                    _ => {}
                },
                KeyCode::BackTab => match self.overlay.as_mut() {
                    Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => {
                        d.confirm_delete_focus_previous()
                    }
                    Some(Overlay::EditSelection(d)) => d.confirm_delete_focus_previous(),
                    _ => {}
                },
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let confirmed = matches!(&self.overlay,
                        Some(Overlay::Edit(d)) | Some(Overlay::Add(d))
                            if d.confirm_delete_is_confirmed())
                        || matches!(&self.overlay,
                        Some(Overlay::EditSelection(d)) if d.confirm_delete_is_confirmed());
                    if confirmed {
                        self.delete_selected_register().await;
                    } else {
                        match self.overlay.as_mut() {
                            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => {
                                d.close_confirm_delete()
                            }
                            Some(Overlay::EditSelection(d)) => d.close_confirm_delete(),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return false;
        }

        let has_sub = matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.has_sub_dialog())
            || matches!(&self.overlay,
            Some(Overlay::Edit(d)) if d.has_sub_dialog())
            || matches!(&self.overlay,
            Some(Overlay::Add(d)) if d.has_sub_dialog());
        if has_sub {
            match self.overlay.as_mut() {
                Some(Overlay::EditSelection(d)) => match code {
                    KeyCode::Esc => d.close_add_dialog(),
                    KeyCode::Enter => d.confirm_add_dialog(),
                    KeyCode::Tab => d.add_dialog_focus_next(),
                    KeyCode::BackTab => d.add_dialog_focus_previous(),
                    _ => d.add_dialog_handle_events(modifiers, code),
                },
                Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => match code {
                    KeyCode::Esc => d.close_add_dialog(),
                    KeyCode::Enter => d.confirm_add_dialog(),
                    KeyCode::Tab => d.add_dialog_focus_next(),
                    KeyCode::BackTab => d.add_dialog_focus_previous(),
                    _ => d.add_dialog_handle_events(modifiers, code),
                },
                _ => {}
            }
            return false;
        }

        // Any keystroke reaching the main dialog body clears a pending name-conflict error so it
        // disappears as the user edits the name. A confirm re-checks and re-sets it below.
        match self.overlay.as_mut() {
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => d.clear_name_error(),
            Some(Overlay::EditSelection(d)) => d.clear_name_error(),
            _ => {}
        }

        // Check whether the code input or confirm button is currently focused in the edit dialogs.
        let update_script_focused = matches!(&self.overlay,
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) if d.is_update_script_focused())
            || matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.is_update_script_focused());
        let confirm_button_focused = matches!(&self.overlay,
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) if d.is_confirm_button_focused())
            || matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.is_confirm_button_focused());
        let delete_register_button_focused = matches!(&self.overlay,
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d))
                if d.is_delete_register_button_focused())
            || matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.is_delete_register_button_focused());

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => self.close_overlay(),
            // Enter inserts a newline when the code field is focused; opens the delete-confirm box
            // when the delete button is focused; otherwise it confirms the dialog.
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if update_script_focused {
                    if let Some(o) = self.overlay.as_mut() {
                        o.handle_events(modifiers, code);
                    }
                } else if delete_register_button_focused {
                    match self.overlay.as_mut() {
                        Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) => d.open_confirm_delete(),
                        Some(Overlay::EditSelection(d)) => d.open_confirm_delete(),
                        _ => {}
                    }
                } else {
                    self.confirm_overlay().await;
                }
            }
            (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                if confirm_button_focused {
                    self.confirm_overlay().await;
                } else if let Some(Overlay::EditSelection(d)) = self.overlay.as_mut() {
                    d.handle_space();
                } else if let Some(Overlay::Edit(d) | Overlay::Add(d)) = self.overlay.as_mut() {
                    d.handle_space();
                } else if let Some(o) = self.overlay.as_mut() {
                    o.handle_events(modifiers, code);
                }
            }
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

        // Auto-switch: EditSelectionDialog → EditInputDialog when all named values are removed.
        // Auto-switch: EditInputDialog → EditSelectionDialog when the first named value is added.
        let new_overlay = match &self.overlay {
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d))
                if !d.pending_named_values.is_empty() =>
            {
                let new_d = d.to_edit_selection_dialog();
                Some(Overlay::EditSelection(new_d))
            }
            Some(Overlay::EditSelection(d)) if d.value.state.values().is_empty() => {
                Some(Overlay::Edit(d.to_edit_input_dialog()))
            }
            _ => None,
        };
        if let Some(o) = new_overlay {
            self.overlay = Some(o);
        }

        false
    }

    /// Confirm the active overlay. Applies only when the dialog fully validates; otherwise it
    /// stays open (Esc cancels). The action is computed before mutating `self`.
    async fn confirm_overlay(&mut self) {
        let action = match &self.overlay {
            Some(Overlay::Setup(d)) => d.resolve().ok().map(|o| match o.device {
                Some((path, device)) => {
                    OverlayAction::CreateModule(o.values, path, Box::new(device))
                }
                None => OverlayAction::ApplySetup(o.values),
            }),
            Some(Overlay::Edit(d)) => d.apply().ok().map(OverlayAction::ApplyEdit),
            Some(Overlay::EditSelection(d)) => d.apply().ok().map(OverlayAction::ApplyEdit),
            Some(Overlay::Add(d)) => d.apply().ok().map(OverlayAction::AddRegister),
            None => None,
        };
        let Some(action) = action else {
            return;
        };

        // Validate name uniqueness before applying an edit or add.
        if let OverlayAction::ApplyEdit(ref edited) = action
            && let Some(tab) = self.tabs.get(self.active)
            && let Some(idx) = tab.table.selected_index()
        {
            let original = tab.table.definitions()[idx].name.clone();
            if edited.name != original && tab.device.definitions.contains_key(&edited.name) {
                let msg = format!("Name '{}' already in use", edited.name);
                match &mut self.overlay {
                    Some(Overlay::Edit(d)) => d.set_name_error(msg),
                    Some(Overlay::EditSelection(d)) => d.set_name_error(msg),
                    _ => {}
                }
                return;
            }
        }
        if let OverlayAction::AddRegister(ref edited) = action
            && let Some(tab) = self.tabs.get(self.active)
            && tab.device.definitions.contains_key(&edited.name)
        {
            let msg = format!("Name '{}' already in use", edited.name);
            if let Some(Overlay::Add(d)) = &mut self.overlay {
                d.set_name_error(msg);
            }
            return;
        }

        match action {
            OverlayAction::CreateModule(values, path, device) => {
                self.create_module(values, path, *device).await
            }
            OverlayAction::ApplySetup(values) => self.apply_setup(values).await,
            OverlayAction::ApplyEdit(edited) => self.apply_edit(edited).await,
            OverlayAction::AddRegister(edited) => self.apply_add(edited).await,
        }
        self.close_overlay();
    }

    /// Open the setup dialog pre-filled from the active tab's instance settings (`:e`).
    pub(super) fn enter_setup(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let timing = crate::module::Module::resolve_timing(&tab.spec, &tab.device);
        let dialog = SetupDialog::edit(
            &tab.spec.name,
            &tab.spec.device,
            tab.spec.role,
            &tab.spec.endpoint,
            (timing.timeout_ms, timing.delay_ms, timing.interval_ms),
            &tab.device.read_ranges,
        );
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the new-module dialog (`:n`/`:new`).
    pub(super) fn enter_new(&mut self) {
        let dialog = SetupDialog::create((DEFAULT_TIMEOUT_MS, DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS));
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the new-module dialog pre-filled with an optional device-config path (`:l`).
    pub(super) fn enter_load(&mut self, path: Option<&str>) {
        let mut dialog =
            SetupDialog::create((DEFAULT_TIMEOUT_MS, DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS));
        if let Some(path) = path {
            dialog.config_path.state.set_input(path.to_string());
            dialog.config_path.state.set_cursor(path.chars().count());
        }
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the edit dialog for the selected register row (Enter in the table).
    pub(super) fn open_edit(&mut self) {
        if self.focus != Focus::Table {
            return;
        }
        let Some(def) = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.table.selected())
        else {
            return;
        };
        let (update_script, current_default) = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.device.definitions.get(&def.name))
            .map(|d| (d.update.as_deref(), d.default.as_ref()))
            .unzip();
        let update_script = update_script.flatten();
        let current_default = current_default.flatten();

        if def.named_values.is_empty() {
            let dialog = EditInputDialog::from_register(
                &def.name,
                &def.description,
                &def.register,
                &def.value,
                update_script,
                current_default,
            );
            self.overlay = Some(Overlay::Edit(dialog));
        } else {
            let dialog = EditSelectionDialog::from_register(
                &def.name,
                &def.description,
                &def.register,
                def.named_values.clone(),
                &def.value,
                &def.raw_value,
                update_script,
                current_default,
            );
            self.overlay = Some(Overlay::EditSelection(dialog));
        }
        self.focus = Focus::Dialog;
    }

    /// Open a blank EditInputDialog to create a new register (`:add`).
    pub(super) fn enter_add(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.overlay = Some(Overlay::Add(EditInputDialog::new()));
        self.focus = Focus::Dialog;
    }

    /// Insert a newly-created register (from the `:add` dialog) into the active tab.
    async fn apply_add(&mut self, edited: EditedRegister) {
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
            length: 1,
            alignment: crate::config::device::AlignmentCfg::default(),
            values: named_values.clone(),
            update: edited.update.as_ref().filter(|s| !s.is_empty()).cloned(),
            description: edited.description.clone(),
            default: edited.default.clone(),
        };
        sync_register_def(&mut def, &edited.register);

        tab.device.definitions.insert(edited.name.clone(), def);
        tab.module.add_register(
            edited.name.clone(),
            edited.description.clone(),
            edited.register.clone(),
            named_values.clone(),
        );

        if let Some((kind, key, range)) = register_mem_binding(&edited.register) {
            tab.module
                .memory()
                .write()
                .await
                .add_ranges(key, &kind, &[range]);
        }

        if let Some(tab) = self.tabs.get(active) {
            tab.module.rebuild_operations().await;
        }

        // Update table view.
        if let Some(tab) = self.tabs.get_mut(active) {
            let mut defs = tab.table.definitions().to_vec();
            defs.push(Definition::new(
                edited.name.clone(),
                edited.description.clone(),
                edited.register.clone(),
                named_values,
            ));
            tab.table.set_definitions(defs);
        }

        // Reload Lua sim when the new register has an update script.
        if edited
            .update
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            && let Some(tab) = self.tabs.get_mut(active)
        {
            let scripts = collect_scripts(&tab.device);
            tab.module.reload_scripts(scripts);
        }

        // Seed a virtual register with a zero/empty value so it shows up before a script or
        // `:set` runs. The configured default or explicit value below will override this.
        if let Address::Virtual = edited.register.address() {
            let seed = crate::module::default_value(&edited.register);
            if let Some(tab) = self.tabs.get(active) {
                tab.module.set_virtual_value(&edited.name, seed).await;
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
    async fn apply_edit(&mut self, edited: EditedRegister) {
        use crate::config::session::Role;
        let active = self.active;
        let mut preserved_value: Option<String> = None;
        let mem_update = if let Some(tab) = self.tabs.get_mut(active)
            && let Some(idx) = tab.table.selected_index()
        {
            let mut defs = tab.table.definitions().to_vec();
            let update = if let Some(slot) = defs.get_mut(idx) {
                let original_name = slot.name.clone();
                let named_values = edited
                    .named_values
                    .clone()
                    .unwrap_or_else(|| slot.named_values.clone());

                // Issue 8: preserve current value on servers when the format changes but address
                // stays the same and the user left the value field blank.
                if tab.spec.role == Role::Server
                    && edited.value.is_none()
                    && slot.register.address() == edited.register.address()
                    && !slot.value.is_empty()
                {
                    preserved_value = Some(slot.value.clone());
                }

                // Issue 9: keep module's register cache in sync so rebuild_operations is correct.
                tab.module.update_register(
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
                    .map(|(kind, key, range)| (tab.module.memory(), key, kind, range));

                // Issue 11: look up by original name, update description, handle rename.
                if let Some(def) = tab.device.definitions.get_mut(&original_name) {
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
                    && let Some(def) = tab.device.definitions.remove(&original_name)
                {
                    tab.device.definitions.insert(edited.name.clone(), def);
                }

                mem_result
            } else {
                None
            };
            tab.table.set_definitions(defs);
            update
        } else {
            None
        };

        if let Some((memory, key, kind, range)) = mem_update {
            memory.write().await.add_ranges(key, &kind, &[range]);
        }

        // Issue 9: refresh client operations after register metadata changed.
        if let Some(tab) = self.tabs.get(active) {
            tab.module.rebuild_operations().await;
        }

        // Reload the Lua sim thread when the edit included a script change so the new script
        // takes effect immediately rather than only on the next module start.
        if edited.update.is_some()
            && let Some(tab) = self.tabs.get_mut(active)
        {
            let scripts = collect_scripts(&tab.device);
            tab.module.reload_scripts(scripts);
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
        self.tabs[active].spec.device = values.config_path.clone();
        self.tabs[active].spec.name = values.name;
        self.tabs[active].spec.role = values.role;
        self.tabs[active].spec.endpoint = values.endpoint.clone();
        self.tabs[active].spec.timeout_ms = values.timeout_ms;
        self.tabs[active].spec.delay_ms = values.delay_ms;
        self.tabs[active].spec.interval_ms = values.interval_ms;
        // Mirror into the device config so `:wd` persists the timing + read ranges too.
        self.tabs[active].device.timeout_ms = values.timeout_ms;
        self.tabs[active].device.delay_ms = values.delay_ms;
        self.tabs[active].device.interval_ms = values.interval_ms;
        self.tabs[active].device.read_ranges = values.read_ranges.clone();

        let timing =
            crate::module::Module::resolve_timing(&self.tabs[active].spec, &self.tabs[active].device);
        if let Err(e) = self.tabs[active]
            .module
            .reconfigure(&values.endpoint, values.role, timing, values.read_ranges)
            .await
        {
            self.tabs[active]
                .module
                .log()
                .write()
                .await
                .write(&format!("Reconfigure failed: {e}"));
            return;
        }
        self.start_module().await;
    }

    /// Build, auto-start and append a new module tab from a confirmed New-module dialog.
    async fn create_module(
        &mut self,
        values: SetupValues,
        device_path: String,
        mut device: crate::config::DeviceConfig,
    ) {
        // Mirror timing + read ranges into the device config so `:wd` persists them.
        device.timeout_ms = values.timeout_ms;
        device.delay_ms = values.delay_ms;
        device.interval_ms = values.interval_ms;
        device.read_ranges = values.read_ranges.clone();
        let spec = ModuleSpec {
            name: values.name,
            device: device_path,
            role: values.role,
            endpoint: values.endpoint,
            timeout_ms: values.timeout_ms,
            delay_ms: values.delay_ms,
            interval_ms: values.interval_ms,
        };
        let mut module = Module::new(&spec, &device);
        if let Err(e) = module.start().await {
            module
                .log()
                .write()
                .await
                .write(&format!("Failed to start: {e}"));
        }
        self.tabs.push(Tab::from_module(spec, device, module));
        self.active = self.tabs.len() - 1;
    }

    /// Delete the register currently selected in the table, removing it from the device config,
    /// the module's register cache, and the table view, then rebuild operations so it is no longer
    /// polled/served. Closes the overlay afterwards.
    async fn delete_selected_register(&mut self) {
        let active = self.active;
        let name = self
            .tabs
            .get(active)
            .and_then(|tab| tab.table.selected())
            .map(|def| def.name.clone());
        let Some(name) = name else {
            self.close_overlay();
            return;
        };

        if let Some(tab) = self.tabs.get_mut(active) {
            tab.device.definitions.remove(&name);
            tab.module.remove_register_by_name(&name);
            let mut defs = tab.table.definitions().to_vec();
            defs.retain(|d| d.name != name);
            tab.table.set_definitions(defs);
            // Reset the table selection to the first remaining row (or none if empty).
            tab.table.select_first();
        }

        // Drop the deleted register from the read plan.
        if let Some(tab) = self.tabs.get(active) {
            tab.module.rebuild_operations().await;
        }

        // Reload the Lua sim in case the removed register carried an update script.
        if let Some(tab) = self.tabs.get_mut(active) {
            let scripts = collect_scripts(&tab.device);
            tab.module.reload_scripts(scripts);
        }

        self.close_overlay();
    }
}
