//! Key routing for the non-dialog panes: command line, table/log navigation, tab switching.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;

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

    pub(super) fn handle_nav_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
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

    pub(super) fn toggle_compact(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.table.set_compact(!tab.table.compact);
        }
    }

    pub(super) fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub(super) fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }
}
