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

/// On-screen log: a fixed-capacity ring of timestamped lines, optionally mirrored to a file so the
/// full history survives the ring's eviction (the `:log <file>` feature).
pub struct LogRing {
    ring: Ring<(u64, String), LOG_SIZE>,
    /// Append-mode file sink set by `:log <file>`; when present, every line is also persisted.
    sink: Option<std::io::BufWriter<std::fs::File>>,
}

impl LogRing {
    pub fn init() -> Self {
        Self {
            ring: Ring::new(),
            sink: None,
        }
    }

    pub fn write(&mut self, msg: &str) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let line: String = msg.chars().take(LOG_MAX_LINE).collect();
        // Persist to the file sink first (unbounded history), then push into the bounded ring.
        if let Some(writer) = self.sink.as_mut() {
            use std::io::Write;
            let _ = writeln!(writer, "[{}] {line}", format_timestamp(ts));
            let _ = writer.flush();
        }
        self.ring.push((ts, line));
    }

    /// Point the log at a file (append): `base` resolves to `<stem>.<name>.<ext>` next to it via
    /// [`module_log_path`](crate::view::log::module_log_path). `None`, or a file that can't be
    /// opened, disables file logging.
    pub fn set_log_file(&mut self, base: Option<&str>, name: &str) {
        self.sink = base.and_then(|base| {
            let path = crate::view::log::module_log_path(base, name);
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(std::io::BufWriter::new)
        });
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
        // Refresh *every* tab's module, not just the active one: background modules must keep
        // sending/receiving while another tab is shown (e.g. an OCPP CSMS keeps driving its Lua
        // sim so a CS tab sees inbound traffic live, instead of only when the tab is switched).
        for tab in self.tabs.iter_mut() {
            tab.view.refresh().await;
            // A view may request to be replaced (e.g. OCPP role switched in the edit dialog).
            if let Some(new_view) = tab.view.take_replacement() {
                tab.view = new_view;
                tab.log = tab.view.log();
            }
            tab.name = tab.view.name();
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn log_ring_persists_lines_to_file_sink() {
        let dir = std::env::temp_dir();
        let base = dir.join(format!("ferrowl_logring_test_{}.log", std::process::id()));
        let base = base.to_str().unwrap();
        let name = "csms";
        let path = crate::view::log::module_log_path(base, name);
        let _ = std::fs::remove_file(&path);

        let mut ring = LogRing::init();
        ring.set_log_file(Some(base), name);
        ring.write("first line");
        ring.write("second line");
        // Disabling the sink flushes/drops the writer.
        ring.set_log_file(None, name);

        let mut contents = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert!(contents.contains("first line"));
        assert!(contents.contains("second line"));
        // Lines are timestamped.
        assert!(contents.trim_start().starts_with('['));
        let _ = std::fs::remove_file(&path);
    }
}
