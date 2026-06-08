use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ferrowl_log::Log;
use ferrowl_mem::{Kind as MemKind, Memory, Range, Type};
use ferrowl_net::{Command, Key, SlaveKind};
use ferrowl_reg::{Access, Address, Kind, Register};
use ferrowl_ui::traits::HandleEvents;
use ferrowl_ui::{AlternateScreen, COLOR_SCHEME};
use ferrowl_util::convert::{Converter, FileType};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, StatefulWidget},
};
use std::io::Stdout;
use std::time::Duration;

use crate::config::{
    AppConfig, DeviceConfig, ModuleSpec, Role, Session,
    device::{
        AccessCfg, AlignmentCfg, EndianCfg, NamedValue, RegisterDef, ValueType as DevValueType,
    },
};
use crate::dialog::{
    EditInputDialog, EditSelectionDialog, EditedRegister, SetupDialog, SetupValues,
};
use crate::module::Module;
use crate::view::command::{CommandLine, new_command_line};
use crate::view::log::{LogEntry, LogView, format_timestamp, new_log_view};
use crate::view::main::TableHeader;
use crate::view::main::{Definition, TableView, cmp_definitions, column_index};
use crate::view::tabs::render_tabs;
use ferrowl_ui::widgets::Header;

/// How often the UI redraws when no input arrives (drives live value updates).
const REDRAW_INTERVAL: Duration = Duration::from_millis(100);

/// Ring-log dimensions for the on-screen log pane.
pub const LOG_MAX_LINE: usize = 256;
pub const LOG_SIZE: usize = 80;
pub type LogRing = Log<LOG_MAX_LINE, LOG_SIZE>;

/// Which pane currently receives input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Table,
    Log,
    Command,
    Dialog,
}

/// The active modal dialog, if any.
enum Overlay {
    Setup(SetupDialog),
    Edit(EditInputDialog),
    EditSelection(EditSelectionDialog<NamedValue>),
    Add(EditInputDialog),
}

impl Overlay {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self {
            Overlay::Setup(d) => d.render(area, buf),
            Overlay::Edit(d) | Overlay::Add(d) => d.render(area, buf),
            Overlay::EditSelection(d) => d.render(area, buf),
        }
    }

    fn focus_next(&mut self) {
        match self {
            Overlay::Setup(d) => d.focus_next(),
            Overlay::Edit(d) | Overlay::Add(d) => d.focus_next(),
            Overlay::EditSelection(d) => d.focus_next(),
        }
    }

    fn focus_previous(&mut self) {
        match self {
            Overlay::Setup(d) => d.focus_previous(),
            Overlay::Edit(d) | Overlay::Add(d) => d.focus_previous(),
            Overlay::EditSelection(d) => d.focus_previous(),
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self {
            Overlay::Setup(d) => {
                let _ = d.handle_events(modifiers, code);
            }
            Overlay::Edit(d) | Overlay::Add(d) => {
                let _ = d.handle_events(modifiers, code);
            }
            Overlay::EditSelection(d) => {
                let _ = d.handle_events(modifiers, code);
            }
        }
    }
}

/// What confirming the active overlay should do (computed before mutating `self`).
enum OverlayAction {
    CreateModule(SetupValues, String, Box<DeviceConfig>),
    ApplySetup(SetupValues),
    ApplyEdit(EditedRegister),
    AddRegister(EditedRegister),
}

/// Per-module UI state shown under one tab: the owning `Module` plus its register table and
/// log view.
pub struct Tab {
    pub name: String,
    pub spec: ModuleSpec,
    pub device: DeviceConfig,
    pub table: TableView,
    pub module: Module,
    /// Active table ordering for `:order` — `(column index, descending)`, or `None` for
    /// device-definition order. Re-applied each `refresh_snapshot` so live columns stay sorted.
    pub sort: Option<(usize, bool)>,
    log_view: LogView,
}

impl Tab {
    /// Build a tab from a module + the spec it was built from. The register table is populated
    /// from the module's register definitions; live values are filled in by
    /// `App::refresh_snapshot`.
    pub fn from_module(spec: ModuleSpec, device: DeviceConfig, module: Module) -> Self {
        let name = spec.name.clone();
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
            name,
            spec,
            device,
            table: TableView::new(definitions),
            module,
            sort: None,
            log_view: new_log_view(),
        }
    }
}

/// Top-level application: owns the terminal and all module tabs, and runs the async
/// event/redraw loop inside the tokio runtime.
pub struct App {
    screen: AlternateScreen<Stdout>,
    tabs: Vec<Tab>,
    active: usize,
    focus: Focus,
    command: CommandLine,
    overlay: Option<Overlay>,
    app_cfg: AppConfig,
    pending_g: bool,
}

impl App {
    pub fn new(tabs: Vec<Tab>, app_cfg: AppConfig) -> std::io::Result<Self> {
        let (overlay, focus) = if tabs.is_empty() {
            let timing = (app_cfg.timeout_ms, app_cfg.delay_ms, app_cfg.interval_ms);
            (
                Some(Overlay::Setup(SetupDialog::create(timing))),
                Focus::Dialog,
            )
        } else {
            (None, Focus::Table)
        };
        Ok(Self {
            screen: AlternateScreen::new()?,
            tabs,
            active: 0,
            focus,
            command: new_command_line(),
            overlay,
            app_cfg,
            pending_g: false,
        })
    }

    /// Run the async UI loop until the user quits.
    ///
    /// crossterm's `read()` is blocking, so a dedicated reader thread forwards terminal
    /// events over an mpsc channel; the loop races event delivery against a redraw tick
    /// via `timeout`, keeping rendering synchronous while waiting asynchronously.
    pub async fn run(&mut self) -> std::io::Result<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(64);
        std::thread::spawn(move || {
            while let Ok(ev) = event::read() {
                if tx.blocking_send(ev).is_err() {
                    break;
                }
            }
        });

        loop {
            self.refresh_snapshot().await;
            self.draw()?;

            match tokio::time::timeout(REDRAW_INTERVAL, rx.recv()).await {
                Ok(Some(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    if self.handle_key(key.modifiers, key.code).await {
                        break;
                    }
                }
                Ok(Some(_)) => {}  // resize/mouse/etc. — redraw on next iteration
                Ok(None) => break, // reader thread gone
                Err(_) => {}       // tick elapsed — redraw
            }
        }
        Ok(())
    }

    /// Snapshot the active module's log and memory into the views (non-destructive),
    /// auto-following the log tail unless the user is scrolling it.
    async fn refresh_snapshot(&mut self) {
        if self.active >= self.tabs.len() {
            return;
        }
        let active = self.active;
        let follow = self.focus != Focus::Log;

        // Clone shared handles + current rows so no `self.tabs` borrow is held across awaits.
        let (log, memory, defs, virtual_store, sort) = {
            let tab = &self.tabs[active];
            (
                tab.module.log(),
                tab.module.memory(),
                tab.table.definitions().to_vec(),
                tab.module.virtual_store(),
                tab.sort,
            )
        };

        let lines = {
            let guard = log.read().await;
            guard.peak_n(LOG_SIZE).unwrap_or_default()
        };
        let virtual_values = virtual_store.read().await.clone();
        let mut updated = {
            let guard = memory.read().await;
            defs.into_iter()
                .map(|d| decode_definition(d, &guard, &virtual_values))
                .collect::<Vec<_>>()
        };
        // Re-apply the active ordering so live columns (Value/Raw Value) stay sorted.
        if let Some((column, descending)) = sort {
            updated.sort_by(|a, b| cmp_definitions(a, b, column, descending));
        }

        let entries: Vec<LogEntry> = lines
            .into_iter()
            .map(|(ts, msg)| LogEntry {
                timestamp: format_timestamp(ts),
                message: msg.trim_end_matches('\u{0}').to_string(),
            })
            .collect();

        let tab = &mut self.tabs[active];
        tab.log_view.state.set_values(entries);
        if follow {
            tab.log_view.state.move_to_bottom();
        }
        tab.table.set_definitions(updated);
    }

    fn draw(&mut self) -> std::io::Result<()> {
        // Disjoint field borrows so the render closure can hold the view state while
        // `screen.draw` holds `&mut screen`.
        let screen = &mut self.screen;
        let tabs = &mut self.tabs;
        let command = &mut self.command;
        let overlay = self.overlay.as_mut();
        let active = self.active;
        let focus = self.focus;
        screen.draw(|f| render(f, tabs, active, focus, command, overlay))?;
        Ok(())
    }

    /// Returns `true` when the application should quit.
    async fn handle_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        match self.focus {
            Focus::Command => self.handle_command_key(modifiers, code).await,
            Focus::Dialog => self.handle_dialog_key(modifiers, code).await,
            Focus::Table | Focus::Log => self.handle_nav_key(modifiers, code),
        }
    }

    async fn handle_dialog_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        // When either edit dialog has an open add sub-dialog, route all keys into it.
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

        // Check whether the code input or confirm button is currently focused in the edit dialogs.
        let update_script_focused = matches!(&self.overlay,
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) if d.is_update_script_focused())
            || matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.is_update_script_focused());
        let confirm_button_focused = matches!(&self.overlay,
            Some(Overlay::Edit(d)) | Some(Overlay::Add(d)) if d.is_confirm_button_focused())
            || matches!(&self.overlay,
            Some(Overlay::EditSelection(d)) if d.is_confirm_button_focused());

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => self.close_overlay(),
            // Enter inserts a newline when the code field is focused; otherwise it confirms.
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if update_script_focused {
                    if let Some(o) = self.overlay.as_mut() {
                        o.handle_events(modifiers, code);
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
                    Some(Overlay::Edit(d)) => d.error.state = msg,
                    Some(Overlay::EditSelection(d)) => d.error.state = msg,
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
                d.error.state = msg;
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
    fn enter_setup(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let timing = crate::module::Module::resolve_timing(&tab.spec, &tab.device, &self.app_cfg);
        let dialog = SetupDialog::edit(
            &tab.spec.name,
            tab.spec.role,
            &tab.spec.endpoint,
            (timing.timeout_ms, timing.delay_ms, timing.interval_ms),
            &tab.device.read_ranges,
        );
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the new-module dialog (`:n`/`:new`).
    fn enter_new(&mut self) {
        let app = &self.app_cfg;
        let dialog = SetupDialog::create((app.timeout_ms, app.delay_ms, app.interval_ms));
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the edit dialog for the selected register row (Enter in the table).
    fn open_edit(&mut self) {
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
    fn enter_add(&mut self) {
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
        if edited.value.is_none() {
            if let Some(ref default_scalar) = edited.default {
                self.set_value(&edited.name, &default_scalar.to_string()).await;
            }
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

        let app_cfg = self.app_cfg.clone();
        let timing = crate::module::Module::resolve_timing(
            &self.tabs[active].spec,
            &self.tabs[active].device,
            &app_cfg,
        );
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
        let mut module = Module::new(&spec, &device, &self.app_cfg);
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

    fn close_overlay(&mut self) {
        self.overlay = None;
        self.focus = Focus::Table;
    }

    async fn handle_command_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => self.exit_command(),
            KeyCode::Enter => {
                let cmd = self.command.state.input().trim().to_string();
                self.exit_command();
                return self.run_command(&cmd).await;
            }
            _ => {
                let _ = self.command.state.handle_events(modifiers, code);
            }
        }
        false
    }

    fn handle_nav_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        // `gt`/`gT` tab switching: a leading `g` was seen last keystroke.
        if self.pending_g {
            self.pending_g = false;
            match code {
                KeyCode::Char('t') => {
                    self.next_tab();
                    return false;
                }
                KeyCode::Char('T') => {
                    self.prev_tab();
                    return false;
                }
                _ => {}
            }
        }

        match (modifiers, code) {
            (_, KeyCode::Char(':')) => self.enter_command(),
            (_, KeyCode::Enter) => self.open_edit(),
            (_, KeyCode::Tab) => self.toggle_pane(),
            (_, KeyCode::Char(']')) => self.next_tab(),
            (_, KeyCode::Char('[')) => self.prev_tab(),
            (KeyModifiers::NONE, KeyCode::Char('z')) => self.toggle_compact(),
            (KeyModifiers::NONE, KeyCode::Char('g')) => {
                self.pending_g = true;
                self.forward_nav(modifiers, code); // `g` still scrolls to top in the table
            }
            (KeyModifiers::SHIFT, KeyCode::Char('g'))
            | (KeyModifiers::NONE, KeyCode::Char('G'))
            | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                self.forward_nav(modifiers, code); // `g` still scrolls to top in the table
            }
            _ => self.forward_nav(modifiers, code),
        }
        false
    }

    fn forward_nav(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        match self.focus {
            Focus::Table => {
                let _ = tab.table.handle_events(modifiers, code);
            }
            Focus::Log => {
                let _ = tab.log_view.state.handle_events(modifiers, code);
            }
            Focus::Command | Focus::Dialog => {}
        }
    }

    fn enter_command(&mut self) {
        self.focus = Focus::Command;
        self.command.state.set_input(String::new());
        self.command.state.set_cursor(0);
        self.command.state.set_focused(true);
    }

    fn exit_command(&mut self) {
        self.command.state.set_focused(false);
        self.command.state.set_input(String::new());
        self.command.state.set_cursor(0);
        self.focus = Focus::Table;
    }

    fn toggle_pane(&mut self) {
        self.focus = match self.focus {
            Focus::Log => Focus::Table,
            _ => Focus::Log,
        };
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.log_view.state.set_focused(self.focus == Focus::Log);
        }
    }

    fn toggle_compact(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.table.set_compact(!tab.table.compact);
        }
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
        let mut module = Module::new(&self.tabs[active].spec, &device, &self.app_cfg);
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

    fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// Execute a parsed `:` command. Returns `true` when the app should quit.
    async fn run_command(&mut self, input: &str) -> bool {
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
                    }
                } else {
                    return false;
                };
                self.log_active(format!("{} {}", self.active, msg)).await;
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
                let path = path.unwrap_or_else(|| {
                    self.tabs
                        .get(self.active)
                        .map(|t| t.spec.device.clone())
                        .unwrap_or_else(|| "device.toml".to_string())
                });
                match self.save_device(&path) {
                    Ok(()) => {
                        self.log_active(format!("Saved device config to {path}"))
                            .await
                    }
                    Err(e) => self.log_active(format!("Save failed: {e}")).await,
                }
            }
            Cmd::Log(file) => match file {
                Some(file) => {
                    self.app_cfg.log_file = Some(file.clone());
                    for tab in &self.tabs {
                        tab.module.set_log_base(Some(&file));
                    }
                    self.log_active(format!("Logging to files based on {file}"))
                        .await;
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

    /// Open the new-module dialog pre-filled with an optional device-config path (`:l`).
    fn enter_load(&mut self, path: Option<&str>) {
        let app = &self.app_cfg;
        let mut dialog = SetupDialog::create((app.timeout_ms, app.delay_ms, app.interval_ms));
        if let Some(path) = path {
            dialog.config_path.state.set_input(path.to_string());
            dialog.config_path.state.set_cursor(path.chars().count());
        }
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    async fn start_module(&mut self) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        let result = self.tabs[active].module.start().await;
        let msg = match result {
            Ok(()) => format!("Started on {}", self.tabs[active].spec.endpoint),
            Err(e) => format!("Start failed: {e}"),
        };
        self.tabs[active].module.log().write().await.write(&msg);
    }

    async fn stop_module(&mut self) {
        let active = self.active;
        if active >= self.tabs.len() {
            return;
        }
        let result = self.tabs[active].module.stop().await;
        let msg = match result {
            Ok(()) => "Stopped".to_string(),
            Err(e) => format!("Stop failed: {e}"),
        };
        self.tabs[active].module.log().write().await.write(&msg);
    }

    /// Write a value to a register on the active module: local memory for servers, a modbus
    /// write command for clients.
    async fn set_value(&mut self, register_name: &str, value: &str) {
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
                    .set_virtual_value(register_name, value.to_string())
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
                    let mut guard = memory.write().await;
                    guard.write_unchecked(
                        Key {
                            id: SlaveKind {
                                slave_id: slave,
                                kind: register.kind().clone(),
                            },
                        },
                        &Range::new(addr as usize, raw.len()),
                        &raw,
                    )
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
                let command = write_command(&register, slave, addr, &raw);
                let result = self.tabs[self.active].module.send_command(command).await;
                match result {
                    Ok(()) => {
                        // Write-only registers are never polled back, so mirror the value into
                        // local memory immediately so the table reflects what was sent.
                        if *register.access() == Access::WriteOnly {
                            let memory = self.tabs[self.active].module.memory();
                            memory.write().await.write_unchecked(
                                Key {
                                    id: SlaveKind {
                                        slave_id: slave,
                                        kind: register.kind().clone(),
                                    },
                                },
                                &Range::new(addr as usize, raw.len()),
                                &raw,
                            );
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

    /// Append a message to the active module's log.
    async fn log_active(&self, message: String) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.module.log().write().await.write(&message);
        }
    }
}

/// Modbus memory type backing a register.
fn mem_type(register: &Register) -> Type {
    match register.kind() {
        Kind::Coil | Kind::DiscreteInput => Type::Coil,
        Kind::HoldingRegister | Kind::InputRegister => Type::Register,
    }
}

/// (name, script) pairs for every register carrying a non-empty `update` Lua snippet.
fn collect_scripts(device: &crate::config::DeviceConfig) -> Vec<(String, String)> {
    device
        .definitions
        .iter()
        .filter_map(|(name, def)| {
            def.update
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| (name.clone(), s.clone()))
        })
        .collect()
}

/// Memory binding `(kind, key, range)` backing a fixed-address register, or `None` if virtual.
fn register_mem_binding(register: &Register) -> Option<(MemKind, Key<SlaveKind>, Range)> {
    let Address::Fixed(addr) = register.address() else {
        return None;
    };
    let ty = mem_type(register);
    let kind = match register.kind() {
        Kind::Coil | Kind::HoldingRegister => MemKind::ReadWrite(ty),
        Kind::DiscreteInput | Kind::InputRegister => MemKind::Read(ty),
    };
    let key = Key {
        id: SlaveKind {
            slave_id: *register.slave_id(),
            kind: register.kind().clone(),
        },
    };
    Some((
        kind,
        key,
        Range::new(*addr as usize, register.format().width()),
    ))
}

/// Build the appropriate write command for a client, based on the register kind/width.
fn write_command(register: &Register, slave: u8, addr: u16, raw: &[u16]) -> Command {
    match register.kind() {
        Kind::Coil | Kind::DiscreteInput => {
            if raw.len() == 1 {
                Command::WriteSingleCoil(slave, addr, raw[0] != 0)
            } else {
                Command::WriteMultipleCoils(slave, addr, raw.iter().map(|v| *v != 0).collect())
            }
        }
        Kind::HoldingRegister | Kind::InputRegister => {
            if raw.len() == 1 {
                Command::WriteSingleRegister(slave, addr, raw[0])
            } else {
                Command::WriteMultipleRegister(slave, addr, raw.to_vec())
            }
        }
    }
}

/// Decode one register's live value from the module memory snapshot.
fn decode_definition(
    mut d: Definition,
    memory: &Memory<Key<SlaveKind>>,
    virtual_values: &std::collections::HashMap<String, String>,
) -> Definition {
    match d.register.address() {
        Address::Fixed(addr) => {
            let width = d.register.format().width();
            let key = Key {
                id: SlaveKind {
                    slave_id: *d.register.slave_id(),
                    kind: d.register.kind().clone(),
                },
            };
            let raw = memory
                .read_unchecked(key, &Range::new(*addr as usize, width))
                .unwrap_or_else(|| vec![0; width]);
            d.value = match d.register.decode(&raw) {
                Ok(v) => format!("{v}"),
                Err(_) => "Error".to_string(),
            };
            d.raw_value = raw_hex(&raw);
        }
        Address::Virtual => {
            // No Modbus address: value comes from the virtual store (Lua sim / server `:set`);
            // derive the raw view by re-encoding it through the register's format.
            match virtual_values.get(&d.name) {
                Some(v) => {
                    d.value = v.clone();
                    d.raw_value = d
                        .register
                        .encode(v)
                        .map(|raw| raw_hex(&raw))
                        .unwrap_or_default();
                }
                None => {
                    d.value.clear();
                    d.raw_value.clear();
                }
            }
        }
    }
    d
}

/// Format register words as `[aaaa bbbb …]` lowercase hex for the table's raw column.
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

/// Sync the mutable `RegisterDef` fields (address, format, access, kind) from an edited
/// `Register`. Named values are handled separately in `apply_edit`.
fn sync_register_def(def: &mut RegisterDef, register: &Register) {
    use ferrowl_reg::Format;

    def.slave_id = *register.slave_id();
    def.access = match register.access() {
        Access::ReadOnly => AccessCfg::ReadOnly,
        Access::WriteOnly => AccessCfg::WriteOnly,
        Access::ReadWrite => AccessCfg::ReadWrite,
    };
    def.read_code = match register.kind() {
        Kind::Coil => 1,
        Kind::DiscreteInput => 2,
        Kind::HoldingRegister => 4,
        Kind::InputRegister => 3,
    };
    match register.address() {
        Address::Fixed(addr) => {
            def.address = Some(*addr);
            def.is_virtual = false;
        }
        Address::Virtual => {
            def.address = None;
            def.is_virtual = true;
        }
    }
    // Every numeric format carries the same (endian, resolution) payload.
    macro_rules! numeric {
        ($vt:ident, $e:expr, $r:expr) => {{
            def.value_type = DevValueType::$vt;
            def.endian = endian_cfg($e);
            def.resolution = $r.0;
        }};
    }
    match register.format() {
        Format::U8((e, r)) => numeric!(U8, e, r),
        Format::U16((e, r)) => numeric!(U16, e, r),
        Format::U32((e, r)) => numeric!(U32, e, r),
        Format::U64((e, r)) => numeric!(U64, e, r),
        Format::U128((e, r)) => numeric!(U128, e, r),
        Format::I8((e, r)) => numeric!(I8, e, r),
        Format::I16((e, r)) => numeric!(I16, e, r),
        Format::I32((e, r)) => numeric!(I32, e, r),
        Format::I64((e, r)) => numeric!(I64, e, r),
        Format::I128((e, r)) => numeric!(I128, e, r),
        Format::F32((e, r)) => numeric!(F32, e, r),
        Format::F64((e, r)) => numeric!(F64, e, r),
        Format::Ascii((align, width)) => {
            def.value_type = DevValueType::Ascii;
            def.alignment = match align {
                ferrowl_reg::format::Alignment::Left => AlignmentCfg::Left,
                ferrowl_reg::format::Alignment::Right => AlignmentCfg::Right,
            };
            def.length = width.0;
        }
    }
}

fn endian_cfg(e: &ferrowl_reg::format::Endian) -> EndianCfg {
    match e {
        ferrowl_reg::format::Endian::Big => EndianCfg::Big,
        ferrowl_reg::format::Endian::Little => EndianCfg::Little,
    }
}

fn render(
    frame: &mut Frame,
    tabs: &mut [Tab],
    active: usize,
    focus: Focus,
    command: &mut CommandLine,
    overlay: Option<&mut Overlay>,
) {
    let area = frame.area();
    let [tabs_area, table_area, log_area, cmd_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(10),
        Constraint::Length(1),
    ])
    .areas(area);

    let buf = frame.buffer_mut();
    buf.set_style(area, Style::default().bg(COLOR_SCHEME.bg));

    let names: Vec<String> = tabs.iter().map(|t| t.name.clone()).collect();
    render_tabs(&names, active, tabs_area, buf);

    if let Some(tab) = tabs.get_mut(active) {
        tab.table.render(table_area, buf);
        StatefulWidget::render(&tab.log_view.widget, log_area, buf, &mut tab.log_view.state);
    }

    render_command(command, focus, cmd_area, buf);
    if focus == Focus::Command {
        render_command_help(cmd_area, buf);
    }

    // Overlay dialog (drawn last; it clears its own area).
    if let Some(dialog) = overlay {
        dialog.render(area, buf);
    }
}

fn render_command_help(cmd_area: Rect, buf: &mut Buffer) {
    const COLS: &[(&str, &str)] = &[
        (":q | :quit", "quit tab"),
        (":qa | :qall", "quit all tabs"),
        (":e | :edit", "edit module setup"),
        (":n | :new", "new module tab"),
        (":l | :load [path]", "load device config"),
        (":a | :add", "add register to device"),
        (":start", "start module"),
        (":stop", "stop module"),
        (":restart", "restart module"),
        (":set <reg> <val>", "write register value"),
        (":s | :save | :w | :write [path]", "save session"),
        (":wd | :write-device [path]", "save device config"),
        (":log [file]", "set log file"),
        (":lua start|stop", "start|stop lua execution"),
        (":reload", "reload device config"),
        (":compact", "toggle compact mode"),
        (":order [col] [asc|desc]", "sort table by column"),
    ];
    let popup_w: u16 = 62;
    let popup_h: u16 = COLS.len() as u16 + 2;
    let x = cmd_area.x;
    let y = cmd_area.y.saturating_sub(popup_h);
    let popup = Rect {
        x,
        y,
        width: popup_w.min(cmd_area.width),
        height: popup_h,
    };

    ratatui::prelude::Widget::render(Clear, popup, buf);
    let block = Block::bordered().style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg));
    let inner = block.inner(popup);
    ratatui::prelude::Widget::render(block, popup, buf);

    let lines: Vec<Line> = COLS
        .iter()
        .map(|(cmd, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{cmd:<34}"),
                    Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg),
                ),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
                ),
            ])
        })
        .collect();
    ratatui::prelude::Widget::render(
        Paragraph::new(lines).style(Style::default().bg(COLOR_SCHEME.bg)),
        inner,
        buf,
    );
}

fn render_command(command: &mut CommandLine, focus: Focus, area: Rect, buf: &mut Buffer) {
    if focus == Focus::Command {
        buf.set_string(
            area.x,
            area.y,
            ":",
            Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg),
        );
        let input_area = Rect {
            x: area.x.saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(1),
            height: area.height,
        };
        StatefulWidget::render(&command.widget, input_area, buf, &mut command.state);
    } else {
        buf.set_style(area, Style::default().bg(COLOR_SCHEME.bg));
        buf.set_string(
            area.x,
            area.y,
            "  :  command    q  quit    Tab  table/log    ] [  tabs    gt gT  tabs",
            Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
        );
    }
}
