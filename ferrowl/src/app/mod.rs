//! Top-level application state and the async event/redraw loop.
//!
//! Submodules split the `App` impl by concern: key routing ([`keys`]), overlay/dialog
//! lifecycle ([`overlay`]), `:` command execution ([`commands`]) and frame rendering
//! ([`mod@render`]).

mod commands;
mod keys;
mod overlay;
mod render;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ferrowl_ring::Ring;
use ferrowl_ui::AlternateScreen;
use ratatui::{buffer::Buffer, layout::Rect};
use std::io::Stdout;
use std::time::Duration;

use crate::module::type_descriptor::SetupView;
use crate::module::type_select::TypeSelectDialog;
use crate::module::view::{ModuleView, SharedLog};
use crate::view::command::{CommandLine, new_command_line};
use crate::view::log::{LogEntry, LogView, format_timestamp, new_log_view};

use render::render;

/// How often the UI redraws when no input arrives (drives live value updates).
const REDRAW_INTERVAL: Duration = Duration::from_millis(100);

/// Ring-log dimensions for the on-screen log pane.
pub const LOG_MAX_LINE: usize = 256;
pub const LOG_SIZE: usize = 80;

/// On-screen log: a fixed-capacity ring of timestamped lines.
pub struct LogRing {
    ring: Ring<(u64, String), LOG_SIZE>,
}

impl LogRing {
    pub fn init() -> Self {
        Self { ring: Ring::new() }
    }

    pub fn write(&mut self, msg: &str) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let line: String = msg.chars().take(LOG_MAX_LINE).collect();
        self.ring.push((ts, line));
    }

    pub fn peek_n(&self, n: usize) -> Vec<(u64, String)> {
        self.ring.peek_n(n).into_iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.ring.clear();
    }
}

/// Which pane currently receives input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Table,
    Log,
    Command,
    Dialog,
}

/// The active modal creation dialog (`:new`/`:load`), if any.
///
/// `:new` opens [`Overlay::TypeSelect`] first; confirming it swaps in
/// [`Overlay::Creation`] for the chosen module type's setup dialog.
enum Overlay {
    TypeSelect(Box<TypeSelectDialog>),
    Creation(Box<dyn SetupView>),
}

impl Overlay {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self {
            Overlay::TypeSelect(d) => d.render(area, buf),
            Overlay::Creation(sv) => sv.render(area, buf),
        }
    }

    fn focus_next(&mut self) {
        match self {
            Overlay::TypeSelect(_) => {}
            Overlay::Creation(sv) => sv.focus_next(),
        }
    }

    fn focus_previous(&mut self) {
        match self {
            Overlay::TypeSelect(_) => {}
            Overlay::Creation(sv) => sv.focus_previous(),
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self {
            Overlay::TypeSelect(d) => d.handle_events(modifiers, code),
            Overlay::Creation(sv) => sv.handle_events(modifiers, code),
        }
    }
}

/// Per-module UI state shown under one tab.
pub struct Tab {
    pub name: String,
    pub log: SharedLog,
    pub log_view: LogView,
    pub view: Box<dyn ModuleView>,
}

impl Tab {
    pub fn new_from_view(name: String, view: Box<dyn ModuleView>) -> Self {
        let log = view.log();
        Self {
            name,
            log,
            log_view: new_log_view(),
            view,
        }
    }
}

pub enum KeyMode {
    CtrlWin,
    CtrlTab,
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
    keymode: Option<KeyMode>,
}

impl App {
    pub fn new(tabs: Vec<Tab>) -> std::io::Result<Self> {
        let (overlay, focus) = if tabs.is_empty() {
            (
                Some(Overlay::TypeSelect(Box::new(TypeSelectDialog::new()))),
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
            keymode: None,
        })
    }

    /// Run the async UI loop until the user quits.
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
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {}
            }
        }
        Ok(())
    }

    async fn refresh_snapshot(&mut self) {
        if self.active >= self.tabs.len() {
            return;
        }
        let active = self.active;
        let follow = self.focus != Focus::Log;

        let log = self.tabs[active].log.clone();
        let lines = {
            let guard = log.read().await;
            guard.peek_n(LOG_SIZE)
        };

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
        tab.view.refresh().await;
        tab.name = tab.view.name();
    }

    fn draw(&mut self) -> std::io::Result<()> {
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

    fn close_overlay(&mut self) {
        self.overlay = None;
        self.focus = Focus::Table;
    }

    async fn log_active(&self, message: String) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.log.write().await.write(&message);
        }
    }
}
