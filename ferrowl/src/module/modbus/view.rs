use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_codec::{Address, Value};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::{Memory, Range};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::HandleEvents;
use ratatui::Frame;
use ratatui::layout::Rect;

use ferrowl_ui::widgets::Header;

use crate::config::device::NamedValue;
use crate::config::{DeviceConfig, ModuleSpec};
use crate::module::modbus::dialog::{
    EditInputDialog, EditSelectionDialog, EditedRegister, SubDialogs,
};
use crate::module::modbus::table::{
    Definition, TableHeader, TableView, cmp_definitions, column_index,
};
use crate::module::view::{
    CommandDescriptor, CommandResult, ModuleView, PendingViewAction, SharedLog,
};

use super::Module;

/// Internal overlay state owned by `ModbusModuleView`.
enum ModbusOverlay {
    Edit(EditInputDialog),
    EditSelection(EditSelectionDialog<NamedValue>),
    Add(EditInputDialog),
}

impl ModbusOverlay {
    fn render(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.render(area, buf),
            ModbusOverlay::EditSelection(d) => d.render(area, buf),
        }
    }

    fn focus_next(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.focus_next(),
            ModbusOverlay::EditSelection(d) => d.focus_next(),
        }
    }

    fn focus_previous(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.focus_previous(),
            ModbusOverlay::EditSelection(d) => d.focus_previous(),
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => {
                let _ = d.handle_events(modifiers, code);
            }
            ModbusOverlay::EditSelection(d) => {
                let _ = d.handle_events(modifiers, code);
            }
        }
    }

    fn clear_name_error(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.clear_name_error(),
            ModbusOverlay::EditSelection(d) => d.clear_name_error(),
        }
    }

    fn has_confirm_delete(&self) -> bool {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.has_confirm_delete(),
            ModbusOverlay::EditSelection(d) => d.has_confirm_delete(),
        }
    }

    fn confirm_delete_is_confirmed(&self) -> bool {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.confirm_delete_is_confirmed(),
            ModbusOverlay::EditSelection(d) => d.confirm_delete_is_confirmed(),
        }
    }

    fn close_confirm_delete(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.close_confirm_delete(),
            ModbusOverlay::EditSelection(d) => d.close_confirm_delete(),
        }
    }

    fn open_confirm_delete(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.open_confirm_delete(),
            ModbusOverlay::EditSelection(d) => d.open_confirm_delete(),
        }
    }

    fn confirm_delete_focus_next(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.confirm_delete_focus_next(),
            ModbusOverlay::EditSelection(d) => d.confirm_delete_focus_next(),
        }
    }

    fn confirm_delete_focus_previous(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.confirm_delete_focus_previous(),
            ModbusOverlay::EditSelection(d) => d.confirm_delete_focus_previous(),
        }
    }

    fn has_sub_dialog(&self) -> bool {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.has_sub_dialog(),
            ModbusOverlay::EditSelection(d) => d.has_sub_dialog(),
        }
    }

    fn close_add_dialog(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.close_add_dialog(),
            ModbusOverlay::EditSelection(d) => d.close_add_dialog(),
        }
    }

    fn confirm_add_dialog(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.confirm_add_dialog(),
            ModbusOverlay::EditSelection(d) => d.confirm_add_dialog(),
        }
    }

    fn add_dialog_focus_next(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.add_dialog_focus_next(),
            ModbusOverlay::EditSelection(d) => d.add_dialog_focus_next(),
        }
    }

    fn add_dialog_focus_previous(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.add_dialog_focus_previous(),
            ModbusOverlay::EditSelection(d) => d.add_dialog_focus_previous(),
        }
    }

    fn add_dialog_handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => {
                d.add_dialog_handle_events(modifiers, code)
            }
            ModbusOverlay::EditSelection(d) => d.add_dialog_handle_events(modifiers, code),
        }
    }

    fn is_update_script_focused(&self) -> bool {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.is_update_script_focused(),
            ModbusOverlay::EditSelection(d) => d.is_update_script_focused(),
        }
    }

    fn is_confirm_button_focused(&self) -> bool {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.is_confirm_button_focused(),
            ModbusOverlay::EditSelection(d) => d.is_confirm_button_focused(),
        }
    }

    fn is_delete_register_button_focused(&self) -> bool {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.is_delete_register_button_focused(),
            ModbusOverlay::EditSelection(d) => d.is_delete_register_button_focused(),
        }
    }

    fn handle_space(&mut self) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.handle_space(),
            ModbusOverlay::EditSelection(d) => d.handle_space(),
        }
    }

    fn set_name_error(&mut self, msg: String) {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.set_name_error(msg),
            ModbusOverlay::EditSelection(d) => d.set_name_error(msg),
        }
    }

    fn apply(&self) -> Option<EditedRegister> {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d.apply().ok(),
            ModbusOverlay::EditSelection(d) => d.apply().ok(),
        }
    }

    fn is_add(&self) -> bool {
        matches!(self, ModbusOverlay::Add(_))
    }

    /// Auto-switch EditInputDialog → EditSelectionDialog when named values are added.
    fn maybe_switch_to_selection(&self) -> Option<ModbusOverlay> {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d)
                if !d.pending_named_values.is_empty() =>
            {
                Some(ModbusOverlay::EditSelection(d.to_edit_selection_dialog()))
            }
            _ => None,
        }
    }

    /// Auto-switch EditSelectionDialog → EditInputDialog when all named values are removed.
    fn maybe_switch_to_input(&self) -> Option<ModbusOverlay> {
        match self {
            ModbusOverlay::EditSelection(d) if d.value.state.values().is_empty() => {
                Some(ModbusOverlay::Edit(d.to_edit_input_dialog()))
            }
            _ => None,
        }
    }
}

pub struct ModbusModuleView {
    module: Module,
    pub spec: ModuleSpec,
    pub device: DeviceConfig,
    table: TableView,
    sort: Option<(usize, bool)>,
    overlay: Option<ModbusOverlay>,
    pending: Option<PendingViewAction>,
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
            pending: None,
        }
    }

    pub fn shared_log(&self) -> SharedLog {
        self.module.log()
    }

    /// Open the edit dialog for the currently selected register row.
    pub fn open_edit(&mut self) {
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

    /// Open a blank add-register dialog.
    pub fn open_add(&mut self) {
        self.overlay = Some(ModbusOverlay::Add(EditInputDialog::new()));
    }

    /// Route a key into the internal overlay. Returns the pending action if one was produced.
    fn handle_overlay_key(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let overlay = match &self.overlay {
            Some(o) => o,
            None => return,
        };

        // Delete-confirm sub-dialog takes priority.
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
                            self.pending = Some(PendingViewAction::DeleteRegister(name));
                        }
                    } else {
                        self.overlay.as_mut().unwrap().close_confirm_delete();
                    }
                }
                _ => {}
            }
            return;
        }

        // Named-value add sub-dialog takes priority.
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
            (KeyModifiers::NONE, KeyCode::Char('z')) => self.toggle_compact(),
            _ => {
                self.overlay
                    .as_mut()
                    .unwrap()
                    .handle_events(modifiers, code);
            }
        }

        // Auto-switch Edit ↔ EditSelection.
        let new_overlay = self.overlay.as_ref().and_then(|o| {
            o.maybe_switch_to_selection()
                .or_else(|| o.maybe_switch_to_input())
        });
        if let Some(o) = new_overlay {
            self.overlay = Some(o);
        }
    }

    /// Confirm the active internal overlay: validate and set pending action.
    fn confirm_overlay(&mut self) {
        let Some(overlay) = &self.overlay else { return };
        let is_add = overlay.is_add();
        match overlay.apply() {
            Some(edited) => {
                let current_name = self.table.selected().map(|d| d.name.clone());
                // Duplicate-name check (only for edits that rename).
                if !is_add {
                    if let Some(original) = &current_name {
                        if &edited.name != original
                            && self.device.definitions.contains_key(&edited.name)
                        {
                            let msg = format!("Name '{}' already in use", edited.name);
                            self.overlay.as_mut().unwrap().set_name_error(msg);
                            return;
                        }
                    }
                } else if self.device.definitions.contains_key(&edited.name) {
                    let msg = format!("Name '{}' already in use", edited.name);
                    self.overlay.as_mut().unwrap().set_name_error(msg);
                    return;
                }
                self.overlay = None;
                self.pending = Some(if is_add {
                    PendingViewAction::AddRegister(edited)
                } else {
                    PendingViewAction::EditRegister(edited)
                });
            }
            None => {} // validation failed — keep dialog open
        }
    }
}

impl ModuleView for ModbusModuleView {
    fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        self.table
            .table
            .state
            .set_focused(focused && self.overlay.is_none());
        self.table.render(area, frame.buffer_mut());
        if let Some(overlay) = &mut self.overlay {
            overlay.render(frame.area(), frame.buffer_mut());
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if self.overlay.is_some() {
            self.handle_overlay_key(modifiers, code);
            EventResult::Consumed
        } else {
            self.table.handle_events(modifiers, code)
        }
    }

    fn refresh(&mut self) {
        let memory_arc = self.module.memory();
        let Ok(memory) = memory_arc.try_read() else {
            return;
        };
        let vs_arc = self.module.virtual_store();
        let Ok(virtual_values) = vs_arc.try_read() else {
            return;
        };

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

    fn take_pending(&mut self) -> Option<PendingViewAction> {
        self.pending.take()
    }

    fn handle_command(&mut self, cmd: &str) -> CommandResult {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        match parts.as_slice() {
            ["edit"] => {
                self.open_edit();
                CommandResult::Handled(None)
            }
            ["add"] => {
                self.open_add();
                CommandResult::Handled(None)
            }
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
                    _ => return CommandResult::Unhandled,
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
        }
    }

    fn commands(&self) -> &[CommandDescriptor] {
        &MODBUS_COMMANDS
    }

    fn is_active(&self) -> bool {
        self.module.is_instance_active()
    }

    fn log(&self) -> SharedLog {
        self.module.log()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl ModbusModuleView {
    pub fn module(&self) -> &Module {
        &self.module
    }

    pub fn module_mut(&mut self) -> &mut Module {
        &mut self.module
    }

    pub fn table(&self) -> &TableView {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut TableView {
        &mut self.table
    }

    pub fn toggle_compact(&mut self) {
        self.table.set_compact(!self.table.compact);
    }

    fn apply_order(&mut self, col: &str, descending: bool) -> CommandResult {
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
}

static MODBUS_COMMANDS: [CommandDescriptor; 2] = [
    CommandDescriptor {
        name: ":lua start|stop|status",
        description: "start|stop|status lua simulation",
    },
    CommandDescriptor {
        name: ":order [col] [asc|desc]",
        description: "sort table by column",
    },
];

/// Decode one register's live value from a memory snapshot into its `Definition` row.
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
