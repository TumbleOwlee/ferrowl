use std::io::Stdout;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ferrowl_log::Log;
use ferrowl_mem::{Kind as MemKind, Memory, Range, Type};
use ferrowl_net::{Command, Key};
use ferrowl_reg::{Access, Address, Kind, Register};
use ferrowl_ui::traits::HandleEvents;
use ferrowl_ui::{AlternateScreen, COLOR_SCHEME};
use ferrowl_util::convert::{Converter, FileType};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Style, palette::tailwind},
    widgets::StatefulWidget,
};

use crate::config::{
    AppConfig, DeviceConfig, ModuleSpec, Role, Session,
    device::{AccessCfg, AlignmentCfg, EndianCfg, NamedValue, RegisterDef, ValueType as DevValueType},
};
use crate::dialog::{EditInputDialog, EditSelectionDialog, EditedRegister, SetupDialog, SetupValues};
use crate::module::Module;
use crate::view::command::{CommandLine, new_command_line};
use crate::view::log::{LogEntry, LogView, new_log_view};
use crate::view::main::{Definition, TableView};
use crate::view::tabs::render_tabs;

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
}

impl Overlay {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self {
            Overlay::Setup(d) => d.render(area, buf),
            Overlay::Edit(d) => d.render(area, buf),
            Overlay::EditSelection(d) => d.render(area, buf),
        }
    }

    fn focus_next(&mut self) {
        match self {
            Overlay::Setup(d) => d.focus_next(),
            Overlay::Edit(d) => d.focus_next(),
            Overlay::EditSelection(d) => d.focus_next(),
        }
    }

    fn focus_previous(&mut self) {
        match self {
            Overlay::Setup(d) => d.focus_previous(),
            Overlay::Edit(d) => d.focus_previous(),
            Overlay::EditSelection(d) => d.focus_previous(),
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self {
            Overlay::Setup(d) => {
                let _ = d.handle_events(modifiers, code);
            }
            Overlay::Edit(d) => {
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
    CreateModule(SetupValues, String, DeviceConfig),
    ApplySetup(SetupValues),
    ApplyEdit(EditedRegister),
}

/// Per-module UI state shown under one tab: the owning `Module` plus its register table and
/// log view.
pub struct Tab {
    pub name: String,
    pub spec: ModuleSpec,
    pub device: DeviceConfig,
    pub table: TableView,
    pub module: Module,
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
            .map(|(name, comment, register, values)| {
                Definition::new(name.clone(), comment.clone(), register.clone(), values.clone())
            })
            .collect();
        Self {
            name,
            spec,
            device,
            table: TableView::new(definitions),
            module,
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
            (Some(Overlay::Setup(SetupDialog::create())), Focus::Dialog)
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
        let (log, memory, id, defs) = {
            let tab = &self.tabs[active];
            (
                tab.module.log(),
                tab.module.memory(),
                tab.module.id(),
                tab.table.definitions().to_vec(),
            )
        };

        let lines = {
            let guard = log.read().await;
            guard.peak_n(LOG_SIZE).unwrap_or_default()
        };
        let updated = {
            let guard = memory.read().await;
            defs.into_iter()
                .map(|d| decode_definition(d, &guard, id))
                .collect::<Vec<_>>()
        };

        let entries: Vec<LogEntry> = lines
            .into_iter()
            .map(|l| LogEntry(l.trim_end_matches('\u{0}').to_string()))
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
        // When the EditSelectionDialog has an open add sub-dialog, route all keys into it.
        let has_sub = matches!(&self.overlay, Some(Overlay::EditSelection(d)) if d.has_sub_dialog());
        if has_sub {
            if let Some(Overlay::EditSelection(d)) = self.overlay.as_mut() {
                match code {
                    KeyCode::Esc => d.close_add_dialog(),
                    KeyCode::Enter => d.confirm_add_dialog(),
                    KeyCode::Tab => d.add_dialog_focus_next(),
                    KeyCode::BackTab => d.add_dialog_focus_previous(),
                    _ => d.add_dialog_handle_events(modifiers, code),
                }
            }
            return false;
        }

        match code {
            KeyCode::Esc => self.close_overlay(),
            KeyCode::Enter => self.confirm_overlay().await,
            KeyCode::Char(' ') => {
                if let Some(Overlay::EditSelection(d)) = self.overlay.as_mut() {
                    d.handle_space();
                } else if let Some(o) = self.overlay.as_mut() {
                    o.handle_events(modifiers, code);
                }
            }
            KeyCode::BackTab => {
                if let Some(o) = self.overlay.as_mut() {
                    o.focus_previous();
                }
            }
            KeyCode::Tab => {
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
            Some(Overlay::Setup(d)) => d.resolve().ok().map(|o| match o.device {
                Some((path, device)) => OverlayAction::CreateModule(o.values, path, device),
                None => OverlayAction::ApplySetup(o.values),
            }),
            Some(Overlay::Edit(d)) => d.apply().ok().map(OverlayAction::ApplyEdit),
            Some(Overlay::EditSelection(d)) => d.apply().ok().map(OverlayAction::ApplyEdit),
            None => None,
        };
        let Some(action) = action else {
            return;
        };
        match action {
            OverlayAction::CreateModule(values, path, device) => {
                self.create_module(values, path, device).await
            }
            OverlayAction::ApplySetup(values) => self.apply_setup(values).await,
            OverlayAction::ApplyEdit(edited) => self.apply_edit(edited).await,
        }
        self.close_overlay();
    }

    /// Open the setup dialog pre-filled from the active tab's instance settings (`:e`).
    fn enter_setup(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let dialog = SetupDialog::edit(&tab.spec.name, tab.spec.role, &tab.spec.endpoint);
        self.overlay = Some(Overlay::Setup(dialog));
        self.focus = Focus::Dialog;
    }

    /// Open the new-module dialog (`:n`/`:new`).
    fn enter_new(&mut self) {
        self.overlay = Some(Overlay::Setup(SetupDialog::create()));
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
        if def.named_values.is_empty() {
            let dialog = EditInputDialog::from_register(
                &def.name,
                &def.comment,
                &def.register,
                &def.value,
            );
            self.overlay = Some(Overlay::Edit(dialog));
        } else {
            let dialog = EditSelectionDialog::from_register(
                &def.name,
                &def.comment,
                &def.register,
                def.named_values.clone(),
                &def.value,
            );
            self.overlay = Some(Overlay::EditSelection(dialog));
        }
        self.focus = Focus::Dialog;
    }

    /// Apply edited register metadata to the selected row, then optionally write its value.
    async fn apply_edit(&mut self, edited: EditedRegister) {
        let active = self.active;
        let mem_update = if let Some(tab) = self.tabs.get_mut(active)
            && let Some(idx) = tab.table.selected_index()
        {
            let mut defs = tab.table.definitions().to_vec();
            let update = if let Some(slot) = defs.get_mut(idx) {
                let named_values = edited
                    .named_values
                    .clone()
                    .unwrap_or_else(|| slot.named_values.clone());
                *slot = Definition::new(
                    edited.name.clone(),
                    edited.comment.clone(),
                    edited.register.clone(),
                    named_values,
                );
                if let Address::Fixed(addr) = edited.register.address() {
                    let ty = mem_type(&edited.register);
                    let kind = match edited.register.access() {
                        Access::ReadOnly => MemKind::Read(ty),
                        Access::WriteOnly => MemKind::Write(ty),
                        Access::ReadWrite => MemKind::Combined(ty),
                    };
                    Some((
                        tab.module.memory(),
                        Key {
                            id: tab.module.id(),
                            slave_id: *edited.register.slave_id(),
                        },
                        kind,
                        Range::new(*addr as usize, edited.register.format().width()),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            tab.table.set_definitions(defs);

            // Keep DeviceConfig in sync with the edited register.
            if let Some(def) = tab.device.definitions.get_mut(&edited.name) {
                sync_register_def(def, &edited.register);
                if let Some(nv) = &edited.named_values {
                    def.values = nv.clone();
                }
            }

            update
        } else {
            None
        };

        if let Some((memory, key, kind, range)) = mem_update {
            memory.write().await.add_ranges(key, &kind, &[range]);
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

        let app_cfg = self.app_cfg.clone();
        if let Err(e) = self.tabs[active]
            .module
            .reconfigure(&values.endpoint, values.role, &app_cfg)
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
        device: crate::config::DeviceConfig,
    ) {
        let spec = ModuleSpec {
            name: values.name,
            device: device_path,
            role: values.role,
            endpoint: values.endpoint,
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
            (KeyModifiers::NONE, KeyCode::Char('g')) => {
                self.pending_g = true;
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
        use crate::command::Cmd;
        match crate::command::parse(input) {
            Cmd::Empty => {}
            Cmd::Quit => return true,
            Cmd::Edit => self.enter_setup(),
            Cmd::New => self.enter_new(),
            Cmd::Load(path) => self.enter_load(path.as_deref()),
            Cmd::Start => self.start_module().await,
            Cmd::Stop => self.stop_module().await,
            Cmd::Restart => {
                self.stop_module().await;
                self.start_module().await;
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
                    Ok(()) => self.log_active(format!("Saved device config to {path}")).await,
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
            Cmd::Unknown(name) => self.log_active(format!("Unknown command ':{name}'")).await,
        }
        false
    }

    /// Open the new-module dialog pre-filled with an optional device-config path (`:l`).
    fn enter_load(&mut self, path: Option<&str>) {
        let mut dialog = SetupDialog::create();
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
        let addr = match register.address() {
            Address::Fixed(a) => *a,
            Address::Virtual => {
                self.log_active(format!(":set '{register_name}' is virtual (no address)"))
                    .await;
                return;
            }
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
                let (memory, id) = {
                    let tab = &self.tabs[self.active];
                    (tab.module.memory(), tab.module.id())
                };
                let ok = {
                    let mut guard = memory.write().await;
                    guard.write_unchecked(
                        Key {
                            id,
                            slave_id: slave,
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
        Converter::save(&tab.device, path, ty).map_err(|e| format!("{e:?}"))
    }

    /// Save the current module instances as a session file.
    fn save_session(&self, path: &str) -> Result<(), String> {
        let ty = FileType::from_path(path)
            .ok_or_else(|| format!("unknown format for '{path}' (use .toml or .json)"))?;
        let session = Session {
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
fn decode_definition(mut d: Definition, memory: &Memory<Key<u8>>, id: u8) -> Definition {
    match d.register.address() {
        Address::Fixed(addr) => {
            let width = d.register.format().width();
            let ty = match d.register.kind() {
                Kind::Coil | Kind::DiscreteInput => Type::Coil,
                Kind::HoldingRegister | Kind::InputRegister => Type::Register,
            };
            let key = Key {
                id,
                slave_id: *d.register.slave_id(),
            };
            let raw = memory
                .read(key, &ty, &Range::new(*addr as usize, width))
                .unwrap_or_else(|| vec![0; width]);
            d.value = match d.register.decode(&raw) {
                Ok(v) => format!("{v}"),
                Err(_) => "Error".to_string(),
            };
            let mut raw_str = String::with_capacity(raw.len() * 3 + 4);
            raw_str += "[";
            let mut first = true;
            for v in raw.iter() {
                if !first {
                    raw_str += &format!(" {:02x}", *v);
                } else {
                    raw_str += &format!("{:02x}", *v);
                }
                first = false;
            }
            raw_str += "]";
            d.raw_value = raw_str;
        }
        Address::Virtual => {
            d.value.clear();
            d.raw_value.clear();
        }
    }
    d
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
    if let Address::Fixed(addr) = register.address() {
        def.address = Some(*addr);
        def.is_virtual = false;
    }
    match register.format() {
        Format::U8((e, r)) => {
            def.value_type = DevValueType::U8;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::U16((e, r)) => {
            def.value_type = DevValueType::U16;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::U32((e, r)) => {
            def.value_type = DevValueType::U32;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::U64((e, r)) => {
            def.value_type = DevValueType::U64;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::U128((e, r)) => {
            def.value_type = DevValueType::U128;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::I8((e, r)) => {
            def.value_type = DevValueType::I8;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::I16((e, r)) => {
            def.value_type = DevValueType::I16;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::I32((e, r)) => {
            def.value_type = DevValueType::I32;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::I64((e, r)) => {
            def.value_type = DevValueType::I64;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::I128((e, r)) => {
            def.value_type = DevValueType::I128;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::F32((e, r)) => {
            def.value_type = DevValueType::F32;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
        Format::F64((e, r)) => {
            def.value_type = DevValueType::F64;
            def.endian = endian_cfg(e);
            def.resolution = r.0;
        }
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

    let names: Vec<String> = tabs.iter().map(|t| t.name.clone()).collect();
    render_tabs(&names, active, tabs_area, buf);

    if let Some(tab) = tabs.get_mut(active) {
        tab.table.render(table_area, buf);
        StatefulWidget::render(&tab.log_view.widget, log_area, buf, &mut tab.log_view.state);
    }

    render_command(command, focus, cmd_area, buf);

    // Overlay dialog (drawn last; it clears its own area).
    if let Some(dialog) = overlay {
        dialog.render(area, buf);
    }
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
