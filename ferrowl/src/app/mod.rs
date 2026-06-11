//! Top-level application state and the async event/redraw loop.
//!
//! Submodules split the `App` impl by concern: key routing ([`keys`]), overlay/dialog
//! lifecycle ([`overlay`]), `:` command execution ([`commands`]), frame rendering
//! ([`mod@render`]) and register/config helpers ([`registers`]).

mod commands;
mod keys;
mod overlay;
mod registers;
mod render;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ferrowl_log::Log;
use ferrowl_ui::AlternateScreen;
use ferrowl_ui::traits::HandleEvents;
use ratatui::{buffer::Buffer, layout::Rect};
use std::io::Stdout;
use std::time::Duration;

use crate::config::{
    DeviceConfig, ModuleSpec,
    device::{DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_TIMEOUT_MS, NamedValue},
};
use crate::dialog::{
    EditInputDialog, EditSelectionDialog, EditedRegister, SetupDialog, SetupValues,
};
use crate::module::Module;
use crate::view::command::{CommandLine, new_command_line};
use crate::view::log::{LogEntry, LogView, format_timestamp, new_log_view};
use crate::view::main::{Definition, TableView, cmp_definitions};

use registers::decode_definition;
use render::render;

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
    pending_g: bool,
}

impl App {
    pub fn new(tabs: Vec<Tab>) -> std::io::Result<Self> {
        let (overlay, focus) = if tabs.is_empty() {
            let timing = (DEFAULT_TIMEOUT_MS, DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS);
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
        let online = self
            .tabs
            .get(self.active)
            .is_some_and(|tab| tab.module.is_instance_active());
        let tabs = &mut self.tabs;
        let command = &mut self.command;
        let overlay = self.overlay.as_mut();
        let active = self.active;
        let focus = self.focus;
        screen.draw(|f| render(f, tabs, active, focus, command, online, overlay))?;
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

    fn close_overlay(&mut self) {
        self.overlay = None;
        self.focus = Focus::Table;
    }

    /// Append a message to the active module's log.
    async fn log_active(&self, message: String) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.module.log().write().await.write(&message);
        }
    }
}
