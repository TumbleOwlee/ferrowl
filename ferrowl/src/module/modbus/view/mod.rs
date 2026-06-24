use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_codec::{Address, Value};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::{Memory, Range};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::HandleEvents;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::config::{DeviceConfig, ModuleSpec};
use crate::module::modbus::dialog::{EditInputDialog, EditSelectionDialog};
use crate::module::modbus::setup_dialog::SetupDialog;
use crate::module::modbus::table::{Definition, TableView, cmp_definitions};
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};

use super::Module;

mod mutate;
mod overlay;
use overlay::{ModbusOverlay, PendingAction};

pub struct ModbusModuleView {
    module: Module,
    spec: ModuleSpec,
    device: DeviceConfig,
    table: TableView,
    sort: Option<(usize, bool)>,
    overlay: Option<ModbusOverlay>,
    setup_overlay: Option<SetupDialog>,
    pending: Option<PendingAction>,
    /// Whether this view (its content pane) currently has keyboard focus, set by the owning `Tab`.
    view_focused: bool,
}

impl ModbusModuleView {
    pub fn new(module: Module, spec: ModuleSpec, device: DeviceConfig) -> Self {
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
            overlay: None,
            setup_overlay: None,
            pending: None,
            view_focused: false,
        }
    }

    fn open_edit(&mut self) {
        let Some(def) = self.table.selected().cloned() else {
            return;
        };
        let (update_script, current_default) = self
            .device
            .definitions
            .get(&def.name)
            .map(|d| (d.update.as_deref(), d.default.as_ref()))
            .unzip();
        let update_script = update_script.flatten();
        let current_default = current_default.flatten();
        let unscaled = def.value.clone().unscaled().to_string();
        if def.named_values.is_empty() {
            self.overlay = Some(ModbusOverlay::Edit(EditInputDialog::from_register(
                &def.name,
                &def.description,
                &def.register,
                &unscaled,
                update_script,
                current_default,
            )));
        } else {
            self.overlay = Some(ModbusOverlay::EditSelection(
                EditSelectionDialog::from_register(
                    &def.name,
                    &def.description,
                    &def.register,
                    def.named_values.clone(),
                    &unscaled,
                    &def.raw_value,
                    update_script,
                    current_default,
                ),
            ));
        }
    }

    fn handle_overlay_key(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let overlay = match &self.overlay {
            Some(o) => o,
            None => return,
        };

        if overlay.has_confirm_delete() {
            match code {
                KeyCode::Esc => self.overlay.as_mut().unwrap().close_confirm_delete(),
                KeyCode::Tab => self.overlay.as_mut().unwrap().confirm_delete_focus_next(),
                KeyCode::BackTab => self
                    .overlay
                    .as_mut()
                    .unwrap()
                    .confirm_delete_focus_previous(),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if self.overlay.as_ref().unwrap().confirm_delete_is_confirmed() {
                        let name = self.table.selected().map(|d| d.name.clone());
                        self.overlay = None;
                        if let Some(name) = name {
                            self.pending = Some(PendingAction::Delete(name));
                        }
                    } else {
                        self.overlay.as_mut().unwrap().close_confirm_delete();
                    }
                }
                _ => {}
            }
            return;
        }

        if overlay.has_sub_dialog() {
            match code {
                KeyCode::Esc => self.overlay.as_mut().unwrap().close_add_dialog(),
                KeyCode::Enter => self.overlay.as_mut().unwrap().confirm_add_dialog(),
                KeyCode::Tab => self.overlay.as_mut().unwrap().add_dialog_focus_next(),
                KeyCode::BackTab => self.overlay.as_mut().unwrap().add_dialog_focus_previous(),
                _ => self
                    .overlay
                    .as_mut()
                    .unwrap()
                    .add_dialog_handle_events(modifiers, code),
            }
            return;
        }

        self.overlay.as_mut().unwrap().clear_name_error();

        let update_script_focused = self.overlay.as_ref().unwrap().is_update_script_focused();
        let confirm_button_focused = self.overlay.as_ref().unwrap().is_confirm_button_focused();
        let delete_button_focused = self
            .overlay
            .as_ref()
            .unwrap()
            .is_delete_register_button_focused();

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.overlay = None;
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if update_script_focused {
                    self.overlay
                        .as_mut()
                        .unwrap()
                        .handle_events(modifiers, code);
                } else if delete_button_focused {
                    self.overlay.as_mut().unwrap().open_confirm_delete();
                } else {
                    self.confirm_overlay();
                }
            }
            (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                if confirm_button_focused {
                    self.confirm_overlay();
                } else {
                    self.overlay.as_mut().unwrap().handle_space();
                }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.overlay.as_mut().unwrap().focus_previous();
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.overlay.as_mut().unwrap().focus_next();
            }
            (KeyModifiers::NONE, KeyCode::Char('z')) => {
                self.table.set_compact(!self.table.compact);
            }
            _ => {
                self.overlay
                    .as_mut()
                    .unwrap()
                    .handle_events(modifiers, code);
            }
        }

        let new_overlay = self.overlay.as_ref().and_then(|o| {
            o.maybe_switch_to_selection()
                .or_else(|| o.maybe_switch_to_input())
        });
        if let Some(o) = new_overlay {
            self.overlay = Some(o);
        }
    }

    fn confirm_overlay(&mut self) {
        let Some(overlay) = &self.overlay else { return };
        let is_add = overlay.is_add();
        if let Some(edited) = overlay.apply() {
            let current_name = self.table.selected().map(|d| d.name.clone());
            if !is_add {
                if let Some(original) = &current_name
                    && &edited.name != original
                    && self.device.definitions.contains_key(&edited.name)
                {
                    let msg = format!("Name '{}' already in use", edited.name);
                    self.overlay.as_mut().unwrap().set_name_error(msg);
                    return;
                }
            } else if self.device.definitions.contains_key(&edited.name) {
                let msg = format!("Name '{}' already in use", edited.name);
                self.overlay.as_mut().unwrap().set_name_error(msg);
                return;
            }
            self.overlay = None;
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
        self.overlay.is_some() || self.setup_overlay.is_some()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ferrowl_ui::{COLOR_SCHEME, style::TextStyle, widgets::TextBuilder};
        use ratatui::{
            layout::{Constraint, HorizontalAlignment, Layout},
            widgets::StatefulWidget,
        };

        let [content_area, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        self.table.table.state.set_focused(
            self.view_focused && self.overlay.is_none() && self.setup_overlay.is_none(),
        );
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
                        .fg(if online {
                            COLOR_SCHEME.text_dark
                        } else {
                            COLOR_SCHEME.text
                        })
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

        let full_area = frame.area();
        if let Some(setup) = &mut self.setup_overlay {
            setup.render(full_area, frame.buffer_mut());
        } else if let Some(overlay) = &mut self.overlay {
            overlay.render(full_area, frame.buffer_mut());
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(ref mut setup) = self.setup_overlay {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    self.setup_overlay = None;
                }
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    if let Ok(resolved) = setup.resolve() {
                        let values = resolved.values;
                        self.setup_overlay = None;
                        self.pending = Some(PendingAction::ApplySetup(values));
                    }
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    setup.focus_next();
                }
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    setup.focus_previous();
                }
                _ => {
                    let _ = setup.handle_events(modifiers, code);
                }
            }
            return EventResult::Consumed;
        }

        if self.overlay.is_some() {
            self.handle_overlay_key(modifiers, code);
            EventResult::Consumed
        } else if modifiers == KeyModifiers::NONE && code == KeyCode::Enter {
            self.open_edit();
            EventResult::Consumed
        } else {
            self.table.handle_events(modifiers, code)
        }
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

            let memory_arc = self.module.memory();
            let memory = memory_arc.read().await;
            let vs_arc = self.module.virtual_store();
            let virtual_values = vs_arc.read().await;

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
                let new_module = Module::new(&self.spec, &device);
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
            let timing = Module::resolve_timing(&self.device);
            let dialog = SetupDialog::edit(
                &self.spec.name,
                &self.spec.device,
                self.spec.role,
                &self.spec.endpoint,
                (timing.timeout_ms, timing.delay_ms, timing.interval_ms),
                &self.device.read_ranges,
            );
            self.setup_overlay = Some(dialog);
            return Box::pin(std::future::ready(CommandResult::Handled(None)));
        }

        if trimmed == "add" || trimmed == "a" {
            self.overlay = Some(ModbusOverlay::Add(EditInputDialog::new()));
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
                self.module.set_log_base(Some(&file));
                CommandResult::Handled(Some(format!(
                    "Logging to files based on {file} (':wd' to persist)"
                )))
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

        // Sync commands routed via App (lua, order) — delegate to the sync inner handler.
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let sync_result = match parts.as_slice() {
            ["lua", action] => {
                let msg = match *action {
                    "start" => {
                        self.module.start_lua();
                        if self.module.lua_running() {
                            "Lua simulation started"
                        } else {
                            "No Lua scripts to run"
                        }
                    }
                    "stop" => {
                        self.module.stop_lua();
                        "Lua simulation stopped"
                    }
                    "status" => {
                        if self.module.lua_running() {
                            "Lua simulation is running"
                        } else {
                            "Lua simulation is stopped"
                        }
                    }
                    _ => return Box::pin(std::future::ready(CommandResult::Unhandled)),
                };
                CommandResult::Handled(Some(msg.to_string()))
            }
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

    fn log(&self) -> SharedLog {
        self.module.log()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let mut v = serde_json::to_value(&self.spec).ok()?;
        v.as_object_mut()?.insert("type".into(), "modbus".into());
        Some(v)
    }
}

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
        name: ":lua start|stop|status",
        description: "lua simulation",
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
    use super::{parse_set_args, raw_hex};

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
}
