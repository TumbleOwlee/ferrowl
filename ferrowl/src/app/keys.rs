//! Key routing for the non-dialog panes: command line, table/log navigation, tab switching.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::HandleEvents;

use crate::app::KeyMode;

use super::{App, Focus};

impl App {
    pub(super) async fn handle_command_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> bool {
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

    // Returns true if has to quit
    pub(super) fn handle_nav_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        match (&self.keymode, modifiers, code) {
            // Window switch
            (None, KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.keymode = Some(KeyMode::CtrlWin)
            }
            (Some(KeyMode::CtrlWin), _, KeyCode::Char('j'))
            | (Some(KeyMode::CtrlWin), _, KeyCode::Down)
            | (Some(KeyMode::CtrlWin), _, KeyCode::Char('k'))
            | (Some(KeyMode::CtrlWin), _, KeyCode::Up) => {
                self.keymode = None;
                self.toggle_pane();
            }
            // Tab switch
            (None, KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                self.keymode = Some(KeyMode::CtrlTab)
            }
            (Some(KeyMode::CtrlTab), _, KeyCode::Char('l')) => {
                self.keymode = None;
                self.next_tab();
            }
            (Some(KeyMode::CtrlTab), _, KeyCode::Char('h')) => {
                self.keymode = None;
                self.prev_tab();
            }
            // Command
            (None, _, KeyCode::Char(':'))
                if !self
                    .tabs
                    .get_mut(self.active)
                    .map(|t| t.view.is_overlay_active())
                    .unwrap_or(false) =>
            {
                self.enter_command()
            }
            (_, _, _) => {
                self.keymode = None;
                self.forward_nav(modifiers, code);
            }
        }
        false
    }

    /// Forward a key to the active tab, which dispatches to whichever of its panes (content view or
    /// log) currently holds focus. Returns `true` if consumed.
    fn forward_nav(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        match self.focus {
            Focus::Content => matches!(tab.handle_events(modifiers, code), EventResult::Consumed),
            Focus::Command | Focus::Dialog => false,
        }
    }

    fn enter_command(&mut self) {
        self.set_content_focus(false);
        self.focus = Focus::Command;
        self.command.state.set_input(String::new());
        self.command.state.set_cursor(0);
        self.command.state.set_focused(true);
    }

    fn exit_command(&mut self) {
        self.command.state.set_focused(false);
        self.command.state.set_input(String::new());
        self.command.state.set_cursor(0);
        self.focus = Focus::Content;
        self.set_content_focus(true);
    }

    /// `Ctrl+w` j/k: toggle focus between the active tab's content view and its log pane. Only
    /// reachable while the content surface is focused (the modal layers route keys elsewhere).
    fn toggle_pane(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.focus_next();
        }
    }

    pub(super) fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.set_content_focus(false);
            self.active = (self.active + 1) % self.tabs.len();
            self.set_content_focus(self.focus == Focus::Content);
        }
    }

    pub(super) fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.set_content_focus(false);
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
            self.set_content_focus(self.focus == Focus::Content);
        }
    }
}
