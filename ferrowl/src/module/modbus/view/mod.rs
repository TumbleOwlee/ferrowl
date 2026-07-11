use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_codec::{Address, Value};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::{Memory, Range};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::{HandleEvents, OverlayRoute, SetFocus};
use ferrowl_ui_derive::Overlay;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::config::script::ScriptDef;
use crate::config::{DeviceConfig, ModuleSpec};
use crate::dialog::close_confirm::CloseConfirmEvent;
use crate::dialog::lua_help::ScriptContext;
use crate::dialog::scripts::ScriptDialog;
use crate::module::modbus::dialog::{EditInputDialog, EditSelectionDialog};
use crate::module::modbus::setup_dialog::SetupDialog;
use crate::module::modbus::table::{Definition, TableView, cmp_definitions};
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};

use super::ModbusModule;

mod mutate;
mod overlay;
use overlay::{ModbusOverlay, PendingAction};

/// The single modal overlay over the module view (mutually exclusive by construction). The
/// derive supplies `is_active`/`close`/`take`/`route_keys`; only the setup dialog carries a
/// common-key tag (`focus_cycle`) — its `Esc`/close-confirm handling lives inside the dialog
/// itself, offered to it before `route_keys` runs (see `handle_events`). The register overlay
/// (`ModbusOverlay`, itself a nested Edit/EditSelection/Add dispatch with its own close-confirm
/// and sub-dialog precedence) and the scripts overlay route every key through their own bespoke
/// handling instead, so they carry no tags.
#[derive(Overlay)]
enum ModbusViewOverlay {
    #[overlay(none)]
    None,
    /// Register edit/add overlay (routes every key through `handle_overlay_key`).
    Register(Box<ModbusOverlay>),
    /// Module re-setup dialog.
    #[overlay(focus_cycle)]
    Setup(Box<SetupDialog>),
    /// Lua scripts editor (routes every key through its own `handle_events`).
    Scripts(Box<ScriptDialog>),
}

ferrowl_ui::impl_overlay_keys!(SetupDialog);

pub struct ModbusModuleView {
    module: ModbusModule,
    spec: ModuleSpec,
    device: DeviceConfig,
    table: TableView,
    sort: Option<(usize, bool)>,
    overlay: ModbusViewOverlay,
    pending: Option<PendingAction>,
    /// Whether this view (its content pane) currently has keyboard focus, set by the owning `Tab`.
    view_focused: bool,
}

impl ModbusModuleView {
    pub fn new(module: ModbusModule, spec: ModuleSpec, device: DeviceConfig) -> Self {
        let definitions = module
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
        Self {
            table: TableView::new(definitions),
            module,
            spec,
            device,
            sort: None,
            overlay: ModbusViewOverlay::None,
            pending: None,
            view_focused: false,
        }
    }

    fn open_edit(&mut self) {
        let Some(def) = self.table.selected().cloned() else {
            return;
        };
        let current_default = self
            .device
            .definitions
            .get(&def.name)
            .and_then(|d| d.default.as_ref());
        let unscaled = def.value.clone().unscaled().to_string();
        if def.named_values.is_empty() {
            self.overlay = ModbusViewOverlay::Register(Box::new(ModbusOverlay::Edit(
                EditInputDialog::from_register(
                    &def.name,
                    &def.description,
                    &def.register,
                    &unscaled,
                    current_default,
                ),
            )));
        } else {
            self.overlay = ModbusViewOverlay::Register(Box::new(ModbusOverlay::EditSelection(
                EditSelectionDialog::from_register(
                    &def.name,
                    &def.description,
                    &def.register,
                    def.named_values.clone(),
                    &unscaled,
                    &def.raw_value,
                    current_default,
                ),
            )));
        }
    }

    /// The register-edit/add overlay as a shared reference, if that's the currently active
    /// overlay variant.
    fn register_overlay(&self) -> Option<&ModbusOverlay> {
        match &self.overlay {
            ModbusViewOverlay::Register(o) => Some(o.as_ref()),
            _ => None,
        }
    }

    /// The register-edit/add overlay as a mutable reference, if that's the currently active
    /// overlay variant.
    fn register_overlay_mut(&mut self) -> Option<&mut ModbusOverlay> {
        match &mut self.overlay {
            ModbusViewOverlay::Register(o) => Some(o.as_mut()),
            _ => None,
        }
    }

    fn handle_overlay_key(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let overlay = match self.register_overlay() {
            Some(o) => o,
            None => return,
        };

        if overlay.has_confirm_delete() {
            match code {
                KeyCode::Esc => self.register_overlay_mut().unwrap().close_confirm_delete(),
                KeyCode::Tab => self
                    .register_overlay_mut()
                    .unwrap()
                    .confirm_delete_focus_next(),
                KeyCode::BackTab => self
                    .register_overlay_mut()
                    .unwrap()
                    .confirm_delete_focus_previous(),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if self
                        .register_overlay()
                        .unwrap()
                        .confirm_delete_is_confirmed()
                    {
                        let name = self.table.selected().map(|d| d.name.clone());
                        self.overlay.close();
                        if let Some(name) = name {
                            self.pending = Some(PendingAction::Delete(name));
                        }
                    } else {
                        self.register_overlay_mut().unwrap().close_confirm_delete();
                    }
                }
                _ => {}
            }
            return;
        }

        if overlay.has_sub_dialog() {
            match code {
                KeyCode::Esc => self.register_overlay_mut().unwrap().close_add_dialog(),
                KeyCode::Enter => self.register_overlay_mut().unwrap().confirm_add_dialog(),
                KeyCode::Tab => self.register_overlay_mut().unwrap().add_dialog_focus_next(),
                KeyCode::BackTab => self
                    .register_overlay_mut()
                    .unwrap()
                    .add_dialog_focus_previous(),
                _ => self
                    .register_overlay_mut()
                    .unwrap()
                    .add_dialog_handle_events(modifiers, code),
            }
            return;
        }

        // The close-confirm popup takes precedence once open.
        if self.register_overlay().unwrap().close_confirm_is_active() {
            match self
                .register_overlay_mut()
                .unwrap()
                .close_confirm_handle_key(modifiers, code)
            {
                CloseConfirmEvent::Close => self.overlay.close(),
                CloseConfirmEvent::Dismiss | CloseConfirmEvent::Consumed => {}
            }
            return;
        }

        self.register_overlay_mut().unwrap().clear_name_error();

        let confirm_button_focused = self.register_overlay().unwrap().is_confirm_button_focused();
        let delete_button_focused = self
            .register_overlay()
            .unwrap()
            .is_delete_register_button_focused();

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.register_overlay_mut().unwrap().close_confirm_open();
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if delete_button_focused {
                    self.register_overlay_mut().unwrap().open_confirm_delete();
                } else {
                    self.confirm_overlay();
                }
            }
            (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                if confirm_button_focused {
                    self.confirm_overlay();
                } else {
                    self.register_overlay_mut().unwrap().handle_space();
                }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.register_overlay_mut().unwrap().focus_previous();
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.register_overlay_mut().unwrap().focus_next();
            }
            (KeyModifiers::NONE, KeyCode::Char('z')) => {
                self.table.set_compact(!self.table.compact);
            }
            _ => {
                self.register_overlay_mut()
                    .unwrap()
                    .handle_events(modifiers, code);
            }
        }

        let new_overlay = self.register_overlay().and_then(|o| {
            o.maybe_switch_to_selection()
                .or_else(|| o.maybe_switch_to_input())
        });
        if let Some(o) = new_overlay {
            self.overlay = ModbusViewOverlay::Register(Box::new(o));
        }
    }

    fn confirm_overlay(&mut self) {
        let Some(overlay) = self.register_overlay() else {
            return;
        };
        let is_add = overlay.is_add();
        if let Some(edited) = overlay.apply() {
            let current_name = self.table.selected().map(|d| d.name.clone());
            if !is_add {
                if let Some(original) = &current_name
                    && &edited.name != original
                    && self.device.definitions.contains_key(&edited.name)
                {
                    let msg = format!("Name '{}' already in use", edited.name);
                    self.register_overlay_mut().unwrap().set_name_error(msg);
                    return;
                }
            } else if self.device.definitions.contains_key(&edited.name) {
                let msg = format!("Name '{}' already in use", edited.name);
                self.register_overlay_mut().unwrap().set_name_error(msg);
                return;
            }
            self.overlay.close();
            if is_add {
                self.pending = Some(PendingAction::Add(edited));
            } else {
                let Some(idx) = self.table.selected_index() else {
                    return;
                };
                let original_name = current_name.unwrap_or_default();
                self.pending = Some(PendingAction::Edit {
                    edited,
                    idx,
                    original_name,
                });
            }
        }
    }
}

impl ferrowl_ui::traits::SetFocus for ModbusModuleView {
    fn set_focused(&mut self, focus: bool) {
        self.view_focused = focus;
    }
}

impl ferrowl_ui::traits::IsFocus for ModbusModuleView {
    fn is_focused(&self) -> bool {
        self.view_focused
    }
}

impl ModuleView for ModbusModuleView {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        self.overlay.is_active()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ferrowl_ui::{COLOR_SCHEME, style::TextStyle, widgets::TextBuilder};
        use ratatui::{
            layout::{Constraint, HorizontalAlignment, Layout},
            widgets::StatefulWidget,
        };

        let [content_area, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        self.table
            .table
            .state
            .set_focused(self.view_focused && !self.overlay.is_active());
        self.table.render(content_area, frame.buffer_mut());

        let online = self.module.is_instance_active();
        {
            let buf = frame.buffer_mut();
            let status_widget = TextBuilder::default()
                .horizontal_alignment(HorizontalAlignment::Center)
                .style(TextStyle {
                    general: ratatui::prelude::Style::default()
                        .bg(if online {
                            COLOR_SCHEME.success
                        } else {
                            COLOR_SCHEME.error
                        })
                        .fg(COLOR_SCHEME.text_status)
                        .bold(),
                })
                .build()
                .unwrap();
            let mut label = if online {
                "ONLINE".to_string()
            } else {
                "OFFLINE".to_string()
            };
            StatefulWidget::render(&status_widget, status_area, buf, &mut label);
        }
    }

    fn render_overlay(&mut self, frame: &mut Frame, _area: Rect) {
        let full_area = frame.area();
        match &mut self.overlay {
            ModbusViewOverlay::Scripts(scripts) => scripts.render(full_area, frame.buffer_mut()),
            ModbusViewOverlay::Setup(setup) => setup.render(full_area, frame.buffer_mut()),
            ModbusViewOverlay::Register(overlay) => overlay.render(full_area, frame.buffer_mut()),
            ModbusViewOverlay::None => {}
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if !self.overlay.is_active() {
            return if modifiers == KeyModifiers::NONE && code == KeyCode::Enter {
                self.open_edit();
                EventResult::Consumed
            } else {
                self.table.handle_events(modifiers, code)
            };
        }

        // Setup dialog: offer the key to the dialog first, so its embedded close-confirm popup
        // can consume Esc/Enter/Tab/BackTab while it is open. Only run the default Enter handling
        // below (via the `route_keys`/per-variant match) when the dialog leaves it unhandled.
        if let ModbusViewOverlay::Setup(setup) = &mut self.overlay
            && let EventResult::Consumed = setup.handle_events(modifiers, code)
        {
            if setup.take_close_request() {
                self.overlay.close();
            }
            return EventResult::Consumed;
        }

        // Common keys: `Tab`/`BackTab` cycle focus on the setup dialog (`focus_cycle`). The
        // register/scripts overlays have no common tags — their close/focus handling is bespoke
        // (register: close-confirm precedes Esc; scripts: every key routes through its own
        // handler) — so `route_keys` always returns `Unhandled` for them.
        match self.overlay.route_keys(modifiers, code) {
            OverlayRoute::Closed | OverlayRoute::Cycled => return EventResult::Consumed,
            OverlayRoute::Unhandled => {}
        }

        match &mut self.overlay {
            ModbusViewOverlay::Setup(setup) => {
                if let (KeyModifiers::NONE, KeyCode::Enter) = (modifiers, code)
                    && let Ok(resolved) = setup.resolve()
                {
                    self.pending = Some(PendingAction::ApplySetup(resolved.values));
                    self.overlay.close();
                }
            }
            ModbusViewOverlay::Scripts(dialog) => {
                if dialog.handle_events(modifiers, code) {
                    let ModbusViewOverlay::Scripts(dialog) = self.overlay.take() else {
                        unreachable!("just matched Scripts above")
                    };
                    let (scripts, interval) = dialog.resolve();
                    self.device.scripts = scripts;
                    self.device.script_interval = interval.as_secs_f64();
                    self.module.set_script_interval(interval);
                    self.module
                        .reload_scripts(super::registers::collect_scripts(&self.device));
                }
            }
            ModbusViewOverlay::Register(_) => self.handle_overlay_key(modifiers, code),
            ModbusViewOverlay::None => {}
        }
        EventResult::Consumed
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            if let Some(pending) = self.pending.take() {
                match pending {
                    PendingAction::Add(edited) => self.apply_add(edited).await,
                    PendingAction::Edit {
                        edited,
                        idx,
                        original_name,
                    } => self.apply_edit(edited, idx, original_name).await,
                    PendingAction::Delete(name) => self.delete_register_by_name(name).await,
                    PendingAction::ApplySetup(values) => self.apply_setup(values).await,
                }
            }

            // Acquire the (async) virtual-store guard first so the (sync) memory guard below is
            // never held across an `.await`. Scoped so both guards are dropped before the
            // `.await` further down (script-log snapshot).
            {
                let vs_arc = self.module.virtual_store();
                let virtual_values = vs_arc.read().await;
                let memory_arc = self.module.memory();
                let memory = memory_arc.read();

                let mut updated: Vec<Definition> = self
                    .table
                    .definitions()
                    .iter()
                    .cloned()
                    .map(|d| decode_definition(d, &memory, &virtual_values))
                    .collect();

                if let Some((column, descending)) = self.sort {
                    updated.sort_by(|a, b| cmp_definitions(a, b, column, descending));
                }

                self.table.set_definitions(updated);
            }

            if let ModbusViewOverlay::Scripts(dialog) = &mut self.overlay {
                let entries = crate::dialog::scripts::snapshot_log(
                    &self.module.script_log(),
                    crate::app::LOG_SIZE,
                )
                .await;
                dialog.set_log_entries(entries);
            }
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let trimmed = cmd.trim();

        if trimmed == "start" {
            return Box::pin(async move {
                let role = self.spec.role.to_string();
                let endpoint = self.spec.endpoint.to_string();
                match self.module.start().await {
                    Ok(()) => CommandResult::Handled(Some(format!("Started {role} on {endpoint}"))),
                    Err(e) => CommandResult::Handled(Some(format!("Start {role} failed: {e}"))),
                }
            });
        }

        if trimmed == "stop" {
            return Box::pin(async move {
                let role = self.spec.role.to_string();
                match self.module.stop().await {
                    Ok(()) => CommandResult::Handled(Some(format!("Stopped {role}"))),
                    Err(e) => CommandResult::Handled(Some(format!("Stop {role} failed: {e}"))),
                }
            });
        }

        if trimmed == "restart" {
            return Box::pin(async move {
                let role = self.spec.role.to_string();
                let endpoint = self.spec.endpoint.to_string();
                let _ = self.module.stop().await;
                match self.module.start().await {
                    Ok(()) => {
                        CommandResult::Handled(Some(format!("Restarted {role} on {endpoint}")))
                    }
                    Err(e) => CommandResult::Handled(Some(format!("Restart {role} failed: {e}"))),
                }
            });
        }

        if trimmed == "reload" {
            return Box::pin(async move {
                if self.spec.device.is_empty() {
                    return CommandResult::Handled(Some(
                        "No configuration file path configured. Reload aborted.".into(),
                    ));
                }
                let path = self.spec.device.clone();
                let device = match crate::config::load_device(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        return CommandResult::Handled(Some(format!(
                            ":reload failed to load '{path}': {e}"
                        )));
                    }
                };
                let _ = self.module.stop().await;
                let new_module = ModbusModule::new(&self.spec, &device);
                self.module = new_module;
                self.device = device;
                let defs: Vec<_> = self
                    .module
                    .registers()
                    .iter()
                    .map(|(n, d, r, v)| Definition::new(n.clone(), d.clone(), r.clone(), v.clone()))
                    .collect();
                self.table.set_definitions(defs);
                self.sort = None;
                if let Err(e) = self.module.start().await {
                    return CommandResult::Handled(Some(format!(":reload start error: {e}")));
                }
                CommandResult::Handled(Some(format!(":reload done — '{path}'")))
            });
        }

        if trimmed == "edit" || trimmed == "e" {
            let timing = ModbusModule::resolve_timing(&self.device);
            let dialog = SetupDialog::edit(
                &self.spec.name,
                &self.spec.device,
                self.spec.role,
                &self.spec.endpoint,
                timing,
                &self.device.read_ranges,
            );
            self.overlay = ModbusViewOverlay::Setup(Box::new(dialog));
            return Box::pin(std::future::ready(CommandResult::Handled(None)));
        }

        if trimmed == "add" || trimmed == "a" {
            self.overlay =
                ModbusViewOverlay::Register(Box::new(ModbusOverlay::Add(EditInputDialog::new())));
            return Box::pin(std::future::ready(CommandResult::Handled(None)));
        }

        if trimmed == "script" {
            self.overlay = ModbusViewOverlay::Scripts(Box::new(ScriptDialog::new(
                &self.device.scripts,
                self.device.script_interval_duration(),
                ScriptContext::Modbus,
            )));
            return Box::pin(std::future::ready(CommandResult::Handled(None)));
        }

        if trimmed == "compact" {
            self.table.set_compact(!self.table.compact);
            return Box::pin(std::future::ready(CommandResult::Handled(None)));
        }

        if trimmed == "wd" {
            if self.spec.device.is_empty() {
                return Box::pin(std::future::ready(CommandResult::Handled(Some(
                    "No configuration file path configured.".into(),
                ))));
            }
            let path = self.spec.device.clone();
            let result = self.save_device_to(&path);
            return Box::pin(std::future::ready(result));
        }

        if let Some(path) = trimmed.strip_prefix("wd ") {
            let path = path.trim().to_string();
            let result = self.save_device_to(&path);
            return Box::pin(std::future::ready(result));
        }

        if let Some(file) = trimmed.strip_prefix("log ") {
            let file = file.trim().to_string();
            return Box::pin(async move {
                self.device.log_file = Some(file.clone());
                match self.module.set_log_base(Some(&file)) {
                    Ok(()) => CommandResult::Handled(Some(format!(
                        "Logging to files based on {file} (':wd' to persist)"
                    ))),
                    Err(e) => CommandResult::Handled(Some(format!(
                        "Failed to open log file {file}: {e}"
                    ))),
                }
            });
        }

        if trimmed.starts_with("set") {
            return Box::pin(async move {
                let rest = cmd
                    .trim_start()
                    .split_once(char::is_whitespace)
                    .map(|(_, r)| r.trim_start())
                    .unwrap_or("");
                let (register, value) = parse_set_args(rest);
                if register.is_empty() || value.is_empty() {
                    return CommandResult::Handled(Some(":set requires <register> <value>".into()));
                }
                self.set_register_value(&register, &value).await
            });
        }

        // Sync commands routed via App (order) — delegate to the sync inner handler.
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let sync_result = match parts.as_slice() {
            ["order"] => {
                let original = self
                    .module
                    .registers()
                    .iter()
                    .map(|(n, d, r, v)| Definition::new(n.clone(), d.clone(), r.clone(), v.clone()))
                    .collect();
                self.sort = None;
                self.table.set_definitions(original);
                CommandResult::Handled(Some("Order cleared".to_string()))
            }
            ["order", col] | ["order", col, "asc"] => self.apply_order(col, false),
            ["order", col, "desc"] => self.apply_order(col, true),
            _ => CommandResult::Unhandled,
        };
        Box::pin(std::future::ready(sync_result))
    }

    fn commands(&self) -> &[CommandDescriptor] {
        &MODBUS_COMMANDS
    }

    fn keybinds(&self) -> &[CommandDescriptor] {
        &MODBUS_KEYBINDS
    }

    fn log(&self) -> SharedLog {
        self.module.log()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let mut v = serde_json::to_value(&self.spec).ok()?;
        v.as_object_mut()?.insert("type".into(), "modbus".into());
        Some(v)
    }

    fn scripts(&self) -> Option<&[ScriptDef]> {
        Some(&self.device.scripts)
    }

    fn set_scripts(&mut self, scripts: Vec<ScriptDef>) -> bool {
        self.device.scripts = scripts;
        self.module
            .reload_scripts(super::registers::collect_scripts(&self.device));
        true
    }

    fn module_host(&self) -> Option<std::sync::Arc<dyn ferrowl_lua::module::ModuleHost>> {
        let registers: HashMap<String, ferrowl_codec::Register> = self
            .module
            .registers()
            .iter()
            .map(|(name, _, register, _)| (name.clone(), register.clone()))
            .collect();
        let role = match self.spec.role {
            crate::config::Role::Client => "client",
            crate::config::Role::Server => "server",
        };
        Some(std::sync::Arc::new(crate::registry::ModbusHost {
            memory: self.module.memory(),
            virtual_store: self.module.virtual_store(),
            registers: std::sync::Arc::new(registers),
            role,
        }))
    }
}

static MODBUS_KEYBINDS: [CommandDescriptor; 5] = [
    CommandDescriptor {
        name: "Enter",
        description: "edit selected register",
    },
    CommandDescriptor {
        name: "Enter (dialog)",
        description: "confirm edit",
    },
    CommandDescriptor {
        name: "Space (dialog)",
        description: "press button / toggle",
    },
    CommandDescriptor {
        name: "z (dialog)",
        description: "toggle compact table",
    },
    CommandDescriptor {
        name: "Esc (dialog)",
        description: "close dialog",
    },
];

static MODBUS_COMMANDS: [CommandDescriptor; 12] = [
    CommandDescriptor {
        name: ":e | :edit",
        description: "edit module setup",
    },
    CommandDescriptor {
        name: ":a | :add",
        description: "add register to device",
    },
    CommandDescriptor {
        name: ":start",
        description: "start module",
    },
    CommandDescriptor {
        name: ":stop",
        description: "stop module",
    },
    CommandDescriptor {
        name: ":restart",
        description: "restart module",
    },
    CommandDescriptor {
        name: ":reload",
        description: "reload device config",
    },
    CommandDescriptor {
        name: ":compact",
        description: "toggle compact mode",
    },
    CommandDescriptor {
        name: ":set <reg> <val>",
        description: "write register value",
    },
    CommandDescriptor {
        name: ":wd | :write-device [path]",
        description: "save device config",
    },
    CommandDescriptor {
        name: ":log <file>",
        description: "set log file",
    },
    CommandDescriptor {
        name: ":script",
        description: "manage lua scripts",
    },
    CommandDescriptor {
        name: ":order [col] [asc|desc]",
        description: "sort table by column",
    },
];

fn decode_definition(
    mut d: Definition,
    memory: &Memory<Key<SlaveKey>>,
    virtual_values: &HashMap<String, Value>,
) -> Definition {
    let prev_raw = std::mem::take(&mut d.raw_value);
    match d.register.address() {
        Address::Fixed(addr) => {
            let width = d.register.format().width();
            let key = Key {
                id: SlaveKey {
                    slave_id: *d.register.slave_id(),
                    kind: d.register.kind().clone(),
                },
            };
            let raw = memory
                .read_unchecked(key, &Range::new(*addr as usize, width))
                .unwrap_or_else(|| vec![0; width]);
            d.value = match d.register.decode(&raw) {
                Ok(v) => v,
                Err(_) => Value::Ascii("Error".to_string()),
            };
            d.raw_value = raw_hex(&raw);
        }
        Address::Virtual => match virtual_values.get(&d.name) {
            Some(v) => {
                d.value = v.clone();
                d.raw_value = d
                    .register
                    .encode(&v.clone().unscaled().to_string())
                    .map(|raw| raw_hex(&raw))
                    .unwrap_or_default();
            }
            None => {
                d.value = Value::Ascii(String::new());
                d.raw_value.clear();
            }
        },
    }
    // The first fill-in (empty previous raw) is not a change; later differing
    // decodes stamp the highlight window (see `Definition::cell_styles`).
    if !prev_raw.is_empty() && d.raw_value != prev_raw {
        d.changed_at = Some(std::time::Instant::now());
    }
    d
}

fn raw_hex(raw: &[u16]) -> String {
    let mut out = String::with_capacity(raw.len() * 5 + 2);
    out.push('[');
    for (i, v) in raw.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out += &format!("{v:04x}");
    }
    out.push(']');
    out
}

fn parse_set_args(rest: &str) -> (String, String) {
    if let Some(after) = rest.strip_prefix('"') {
        match after.split_once('"') {
            Some((reg, val)) => (reg.to_string(), val.trim_start().to_string()),
            None => (after.to_string(), String::new()),
        }
    } else {
        match rest.split_once(char::is_whitespace) {
            Some((reg, val)) => (reg.to_string(), val.trim_start().to_string()),
            None => (rest.to_string(), String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModbusModuleView, ModbusViewOverlay, PendingAction, decode_definition, parse_set_args,
        raw_hex,
    };
    use crate::config::{DeviceConfig, Endpoint, ModuleSpec, Role};
    use crate::module::modbus::setup_dialog::SetupValues;
    use crate::module::modbus::table::Definition;
    use crate::module::view::ModuleView;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_codec::format::{BitField, Endian, Format, Resolution};
    use ferrowl_codec::{Access, Address, Kind, RegisterBuilder, Value};
    use ferrowl_modbus::{Key, SlaveKey};
    use ferrowl_store::{CellKind, Memory, Range};
    use ferrowl_ui::EventResult;
    use ratatui::Frame;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::collections::HashMap;

    fn empty_device() -> DeviceConfig {
        DeviceConfig {
            version: None,
            timeout_ms: None,
            delay_ms: None,
            interval_ms: None,
            reconnect: None,
            log_file: None,
            read_ranges: Default::default(),
            definitions: Default::default(),
            script_interval: 1.0,
            scripts: Default::default(),
        }
    }

    fn tcp_server_spec() -> ModuleSpec {
        ModuleSpec {
            name: "test module".into(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
        }
    }

    fn new_view() -> ModbusModuleView {
        let device = empty_device();
        let spec = tcp_server_spec();
        let module = super::super::ModbusModule::new(&spec, &device);
        ModbusModuleView::new(module, spec, device)
    }

    // --- decode_definition -------------------------------------------------

    fn fixed_def() -> Definition {
        let register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(Format::U16((
                Endian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();
        Definition::new("hold".to_string(), "d".to_string(), register, vec![])
    }

    fn virtual_def() -> Definition {
        let register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Virtual)
            .format(Format::U16((
                Endian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();
        Definition::new("virt".to_string(), "d".to_string(), register, vec![])
    }

    #[test]
    fn ut_decode_definition_fixed_reads_memory_word() {
        let def = fixed_def();
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let key = Key {
            id: SlaveKey {
                slave_id: 1,
                kind: Kind::HoldingRegister,
            },
        };
        memory.add_ranges(
            key.clone(),
            &CellKind::ReadWrite(ferrowl_store::CellType::Register),
            std::slice::from_ref(&Range::new(0, 1)),
        );
        memory.write_unchecked(key, &Range::new(0, 1), &[42u16]);
        let empty_vs: HashMap<String, Value> = HashMap::new();
        let decoded = decode_definition(def, &memory, &empty_vs);
        assert!(matches!(decoded.value, Value::U16((42, _))));
        assert_eq!(decoded.raw_value, "[002a]");
    }

    #[test]
    fn ut_decode_definition_fixed_missing_memory_defaults_to_zero() {
        let def = fixed_def();
        let memory = Memory::<Key<SlaveKey>>::default();
        let empty_vs: HashMap<String, Value> = HashMap::new();
        let decoded = decode_definition(def, &memory, &empty_vs);
        assert!(matches!(decoded.value, Value::U16((0, _))));
    }

    #[test]
    fn ut_decode_definition_virtual_uses_store_value() {
        let def = virtual_def();
        let memory = Memory::<Key<SlaveKey>>::default();
        let mut vs: HashMap<String, Value> = HashMap::new();
        vs.insert("virt".into(), Value::U16((9, Resolution(1.0))));
        let decoded = decode_definition(def, &memory, &vs);
        assert!(matches!(decoded.value, Value::U16((9, _))));
        assert_eq!(decoded.raw_value, "[0009]");
    }

    #[test]
    fn ut_decode_definition_virtual_missing_value_is_blank() {
        let def = virtual_def();
        let memory = Memory::<Key<SlaveKey>>::default();
        let empty_vs: HashMap<String, Value> = HashMap::new();
        let decoded = decode_definition(def, &memory, &empty_vs);
        assert!(matches!(decoded.value, Value::Ascii(ref s) if s.is_empty()));
        assert!(decoded.raw_value.is_empty());
    }

    #[test]
    fn ut_decode_definition_first_fill_is_not_a_change() {
        let def = fixed_def();
        let memory = Memory::<Key<SlaveKey>>::default();
        let empty_vs: HashMap<String, Value> = HashMap::new();
        let decoded = decode_definition(def, &memory, &empty_vs);
        assert!(decoded.changed_at.is_none());
        // A second identical decode is not a change either.
        let decoded = decode_definition(decoded, &memory, &empty_vs);
        assert!(decoded.changed_at.is_none());
    }

    #[test]
    fn ut_decode_definition_value_change_stamps_changed_at() {
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let key = Key {
            id: SlaveKey {
                slave_id: 1,
                kind: Kind::HoldingRegister,
            },
        };
        memory.add_ranges(
            key.clone(),
            &CellKind::ReadWrite(ferrowl_store::CellType::Register),
            std::slice::from_ref(&Range::new(0, 1)),
        );
        memory.write_unchecked(key.clone(), &Range::new(0, 1), &[1u16]);
        let empty_vs: HashMap<String, Value> = HashMap::new();
        let decoded = decode_definition(fixed_def(), &memory, &empty_vs);
        assert!(decoded.changed_at.is_none(), "first fill must not stamp");
        memory.write_unchecked(key, &Range::new(0, 1), &[2u16]);
        let decoded = decode_definition(decoded, &memory, &empty_vs);
        assert!(decoded.changed_at.is_some(), "changed value must stamp");
    }

    // --- view construction & key handling -----------------------------------

    #[test]
    fn ut_new_view_starts_with_no_overlay() {
        let view = new_view();
        assert!(!view.is_overlay_active());
    }

    #[test]
    fn ut_enter_on_empty_table_does_not_open_overlay() {
        // No registers selected -> open_edit returns early without an overlay.
        let mut view = new_view();
        let result = view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(result, EventResult::Consumed));
        assert!(!view.is_overlay_active());
    }

    #[test]
    fn ut_scripts_command_opens_overlay_and_close_applies() {
        let mut view = new_view();
        drop(view.handle_command("script"));
        assert!(view.is_overlay_active());
        // Create a script through the dialog: Tab past the interval field to the table, then to
        // the name input (the code editor is skipped while nothing is selected), type a name,
        // Enter creates it, Esc + Enter (confirm-close) closes.
        view.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        view.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        for c in "sim".chars() {
            view.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(!view.is_overlay_active());
        assert_eq!(view.device.scripts.len(), 1);
        assert_eq!(view.device.scripts[0].name, "sim");
        assert!(view.device.scripts[0].enabled);
    }

    #[test]
    fn ut_edit_command_opens_setup_overlay() {
        let mut view = new_view();
        drop(view.handle_command("edit"));
        assert!(view.is_overlay_active());
    }

    #[test]
    fn ut_setup_overlay_tab_cycles_focus_via_derive() {
        // Regression for the `#[derive(Overlay)]` port: `Setup` is tagged `focus_cycle`, so Tab
        // must still advance the dialog's own focus once the dialog itself leaves the key
        // unhandled (no close-confirm open).
        use ferrowl_ui::traits::IsFocus;
        let mut view = new_view();
        drop(view.handle_command("edit"));
        let ModbusViewOverlay::Setup(setup) = &view.overlay else {
            panic!("expected Setup overlay");
        };
        assert!(setup.name.state.is_focused());
        assert!(!setup.config_path.state.is_focused());
        view.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        let ModbusViewOverlay::Setup(setup) = &view.overlay else {
            panic!("expected Setup overlay");
        };
        assert!(!setup.name.state.is_focused());
        assert!(setup.config_path.state.is_focused());
    }

    #[test]
    fn ut_setup_overlay_backtab_cycles_focus_reverse() {
        use ferrowl_ui::traits::IsFocus;
        let mut view = new_view();
        drop(view.handle_command("edit"));
        view.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        view.handle_events(KeyModifiers::NONE, KeyCode::BackTab);
        let ModbusViewOverlay::Setup(setup) = &view.overlay else {
            panic!("expected Setup overlay");
        };
        // BackTab after one Tab lands back on the first field.
        assert!(setup.name.state.is_focused());
        assert!(!setup.config_path.state.is_focused());
    }

    #[test]
    fn ut_register_overlay_swallows_table_navigation_key() {
        // Regression: while the register overlay is open, `Down` must be consumed by the
        // overlay's own dispatch (untagged -> bespoke `handle_overlay_key`), not fall through to
        // the underlying table's selection movement.
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        let before = view.table.selected().map(|d| d.name.clone());
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(view.is_overlay_active());
        view.handle_events(KeyModifiers::NONE, KeyCode::Down);
        let after = view.table.selected().map(|d| d.name.clone());
        assert_eq!(
            before, after,
            "table selection must not move while overlay is open"
        );
    }

    #[test]
    fn ut_enter_still_confirms_edit_dialog_after_offer_first_routing() {
        // Regression for the offer-first key-routing refactor: the setup dialog is now offered
        // every key before the default Esc/Enter/Tab/BackTab handling runs, but Enter must still
        // confirm the dialog and apply the edit exactly as before.
        let mut view = new_view();
        drop(view.handle_command("edit"));
        assert!(view.is_overlay_active());
        let result = view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(result, EventResult::Consumed));
        assert!(!view.is_overlay_active());
        assert!(matches!(view.pending, Some(PendingAction::ApplySetup(_))));
    }

    #[test]
    fn ut_esc_does_not_close_setup_overlay() {
        let mut view = new_view();
        drop(view.handle_command("edit"));
        assert!(view.is_overlay_active());
        // First Esc opens the close-confirm popup instead of closing the overlay outright.
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
        // A second Esc dismisses the confirm popup, leaving the overlay open.
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
    }

    #[test]
    fn ut_esc_then_enter_closes_setup_overlay() {
        let mut view = new_view();
        drop(view.handle_command("edit"));
        assert!(view.is_overlay_active());
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(!view.is_overlay_active());
    }

    #[test]
    fn ut_add_command_opens_add_overlay() {
        let mut view = new_view();
        drop(view.handle_command("add"));
        assert!(view.is_overlay_active());
        // First Esc opens the close-confirm popup; overlay stays open.
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
        // Enter confirms the close-confirm popup, closing the overlay.
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(!view.is_overlay_active());
    }

    #[test]
    fn ut_compact_command_toggles_table_compact_flag() {
        let mut view = new_view();
        assert!(!view.table.compact);
        drop(view.handle_command("compact"));
        assert!(view.table.compact);
    }

    /// All buffer cell symbols joined into one string, for containment assertions.
    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// A device config with one plain fixed register ("hold") and one with named values
    /// ("named"), to drive both edit-overlay flavours.
    fn device_with_defs() -> DeviceConfig {
        use crate::config::device::{
            AccessCfg, AlignmentCfg, EndianCfg, NamedValue, RegisterDef, Scalar,
            ValueType as CfgValueType,
        };

        let base = |address: u16, values: Vec<NamedValue>| RegisterDef {
            slave_id: 1,
            kind: Kind::HoldingRegister,
            address: Some(address),
            is_virtual: false,
            access: AccessCfg::ReadWrite,
            value_type: CfgValueType::U16,
            endian: EndianCfg::Big,
            resolution: 1.0,
            bitmask: None,
            length: 1,
            alignment: AlignmentCfg::Left,
            values,
            update: None,
            description: "desc".into(),
            default: None,
        };

        let mut device = empty_device();
        device.definitions.insert("hold".into(), base(0, vec![]));
        device.definitions.insert(
            "named".into(),
            base(
                1,
                vec![NamedValue {
                    name: "on".into(),
                    value: Scalar::Int(1),
                }],
            ),
        );
        device
    }

    fn view_for(device: DeviceConfig) -> ModbusModuleView {
        let spec = tcp_server_spec();
        let module = super::super::ModbusModule::new(&spec, &device);
        ModbusModuleView::new(module, spec, device)
    }

    #[test]
    fn ut_esc_does_not_close_register_overlay() {
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        // Definitions are BTreeMap-ordered: "hold" (no named values) comes first.
        assert_eq!(view.table.selected().map(|d| d.name.as_str()), Some("hold"));
        let result = view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(result, EventResult::Consumed));
        assert!(view.is_overlay_active());
        // First Esc opens the close-confirm popup; overlay stays open.
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
        assert!(view.register_overlay().unwrap().close_confirm_is_active());
        // A second Esc dismisses the confirm popup, leaving the overlay open.
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
        assert!(!view.register_overlay().unwrap().close_confirm_is_active());
    }

    #[test]
    fn ut_esc_then_enter_closes_register_overlay() {
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(view.is_overlay_active());
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(view.is_overlay_active());
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(!view.is_overlay_active());
    }

    #[test]
    fn ut_colon_in_value_input_types() {
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(view.is_overlay_active());
        // Focus starts on the free-text Value field; `:` must be typed as ordinary text.
        view.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert!(!view.register_overlay().unwrap().close_confirm_is_active());
        assert!(view.is_overlay_active());
    }

    #[test]
    fn ut_confirm_delete_esc_still_cancels() {
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(view.is_overlay_active());
        // Value -> DefaultValue -> AddButton -> ConfirmButton -> DeleteRegisterButton.
        for _ in 0..4 {
            view.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        }
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter); // open confirm-delete
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc); // cancels the sub-dialog only
        assert!(view.is_overlay_active());
        assert!(view.pending.is_none());
    }

    #[test]
    fn ut_named_value_subdialog_esc_still_cancels() {
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        view.handle_events(KeyModifiers::NONE, KeyCode::Down); // "named"
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter); // open selection overlay
        assert!(view.is_overlay_active());
        view.handle_events(KeyModifiers::NONE, KeyCode::Tab); // Value -> AddButton
        view.handle_events(KeyModifiers::NONE, KeyCode::Char(' ')); // open add-named-value sub-dialog
        view.handle_events(KeyModifiers::NONE, KeyCode::Esc); // cancels the sub-dialog only
        assert!(view.is_overlay_active());
    }

    #[test]
    fn ut_enter_on_named_value_register_opens_selection_overlay() {
        let mut view = view_for(device_with_defs());
        view.table.select_first();
        // Move to "named" (second row) which carries named values -> selection dialog.
        view.handle_events(KeyModifiers::NONE, KeyCode::Down);
        assert_eq!(
            view.table.selected().map(|d| d.name.as_str()),
            Some("named")
        );
        view.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(view.is_overlay_active());
        // The selection overlay renders with the "Edit" box title and the named-value label.
        let area = Rect::new(0, 0, 80, 48);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 48)).unwrap();
        term.draw(|f: &mut Frame| view.render_overlay(f, area))
            .unwrap();
        let text = buffer_text(term.backend().buffer());
        assert!(text.contains("Edit"), "missing dialog title:\n{text}");
        assert!(text.contains("on"), "missing named value label:\n{text}");
    }

    #[test]
    fn ut_render_shows_table_and_offline_status() {
        let mut view = view_for(device_with_defs());
        let area = Rect::new(0, 0, 120, 24);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
        term.draw(|f: &mut Frame| view.render(f, area)).unwrap();
        let text = buffer_text(term.backend().buffer());
        // Table title, a register row, and the not-started status line are all drawn.
        assert!(text.contains("Register"), "missing table title:\n{text}");
        assert!(text.contains("hold"), "missing register row:\n{text}");
        assert!(text.contains("OFFLINE"), "missing status line:\n{text}");
    }

    #[test]
    fn ut_render_overlay_add_dialog_shows_box_title_and_fields() {
        let mut view = new_view();
        drop(view.handle_command("add"));
        let area = Rect::new(0, 0, 80, 52);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 52)).unwrap();
        term.draw(|f: &mut Frame| view.render_overlay(f, area))
            .unwrap();
        let text = buffer_text(term.backend().buffer());
        assert!(text.contains("Add"), "missing dialog title:\n{text}");
        assert!(text.contains("Label"), "missing label field:\n{text}");
        assert!(text.contains("CONFIRM"), "missing confirm button:\n{text}");
    }

    #[test]
    fn ut_raw_hex_formats_words_lowercase_space_separated() {
        assert_eq!(raw_hex(&[]), "[]");
        assert_eq!(raw_hex(&[0x0001]), "[0001]");
        assert_eq!(raw_hex(&[0x00a0, 0x0001]), "[00a0 0001]");
        assert_eq!(raw_hex(&[0xffff, 0x0000]), "[ffff 0000]");
    }

    #[test]
    fn ut_parse_set_args_unquoted_splits_on_first_whitespace() {
        assert_eq!(parse_set_args("reg 123"), ("reg".into(), "123".into()));
        // Extra leading whitespace before the value is trimmed.
        assert_eq!(parse_set_args("reg   123"), ("reg".into(), "123".into()));
        // No value -> empty string.
        assert_eq!(parse_set_args("reg"), ("reg".into(), String::new()));
    }

    #[test]
    fn ut_parse_set_args_quoted_name_keeps_inner_spaces() {
        assert_eq!(
            parse_set_args("\"my reg\" 456"),
            ("my reg".into(), "456".into())
        );
        assert_eq!(
            parse_set_args("\"my reg\" hello world"),
            ("my reg".into(), "hello world".into())
        );
        // Quoted name, no value.
        assert_eq!(
            parse_set_args("\"my reg\""),
            ("my reg".into(), String::new())
        );
    }

    #[tokio::test]
    async fn ut_apply_setup_server_role_preserves_existing_reconnect() {
        // Reconnect is hidden/unset (None) for Server-role dialog saves; applying it must not
        // clobber whatever the device config already had for a setting the user never saw.
        let mut view = new_view();
        view.device.reconnect = Some(false);
        let values = SetupValues {
            name: "test module".into(),
            config_path: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
            timeout_ms: None,
            delay_ms: None,
            interval_ms: None,
            reconnect: None,
            read_ranges: Default::default(),
        };
        view.apply_setup(values).await;
        assert_eq!(view.device.reconnect, Some(false));
    }
}
