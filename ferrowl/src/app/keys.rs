//! Key routing for the non-dialog panes: command line, table/log navigation, tab switching.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::{HandleEvents, SetFocus};

use crate::app::{DIGIT_CHORD_TIMEOUT, KeyMode};

use super::{App, Focus};

/// Result of feeding one digit into the `Ctrl+t` tab-jump chord.
#[derive(Debug, PartialEq, Eq)]
enum DigitOutcome {
    /// First digit could still be the start of a valid two-digit index: hold it and wait.
    Wait(usize),
    /// Jump straight to this tab index.
    Jump(usize),
    /// No tab can match; drop the chord.
    Ignore,
}

/// Decide what a newly typed digit `d` means for the tab-jump chord, given any `pending` first
/// digit and the number of tabs `ntabs`. Pure so the chord logic is testable without an `App`.
fn digit_outcome(pending: Option<usize>, d: usize, ntabs: usize) -> DigitOutcome {
    match pending {
        None => {
            // A leading 0 never needs a second digit: every "0x" index is reachable as "x".
            if d != 0 && d * 10 < ntabs {
                DigitOutcome::Wait(d)
            } else if d < ntabs {
                DigitOutcome::Jump(d)
            } else {
                DigitOutcome::Ignore
            }
        }
        Some(first) => {
            let idx = first * 10 + d;
            DigitOutcome::Jump(if idx < ntabs { idx } else { first })
        }
    }
}

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
        // The help dialog is modal: it eats every key so nothing leaks to the view beneath it.
        if self.help_open {
            match code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                    self.help_open = false;
                    self.help_scroll = 0;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.help_scroll = self.help_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1)
                }
                KeyCode::Char('g') => self.help_scroll = 0,
                // Clamped to the last line at render time, which knows the viewport height.
                KeyCode::Char('G') => self.help_scroll = u16::MAX,
                _ => {}
            }
            return false;
        }
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
            // Ctrl+t then a digit: jump straight there, or wait for a second digit if one could
            // still form a valid two-digit index.
            (Some(KeyMode::CtrlTab), _, KeyCode::Char(c)) if c.is_ascii_digit() => {
                let d = c.to_digit(10).unwrap() as usize;
                match digit_outcome(None, d, self.tabs.len()) {
                    DigitOutcome::Wait(first) => {
                        self.keymode = Some(KeyMode::TabDigit {
                            first,
                            deadline: Instant::now() + DIGIT_CHORD_TIMEOUT,
                        });
                    }
                    DigitOutcome::Jump(idx) => {
                        self.keymode = None;
                        self.switch_tab(idx);
                    }
                    DigitOutcome::Ignore => self.keymode = None,
                }
            }
            // Second digit of the chord, within the timeout window.
            (Some(KeyMode::TabDigit { first, .. }), _, KeyCode::Char(c)) if c.is_ascii_digit() => {
                let first = *first;
                let d = c.to_digit(10).unwrap() as usize;
                self.keymode = None;
                if let DigitOutcome::Jump(idx) = digit_outcome(Some(first), d, self.tabs.len()) {
                    self.switch_tab(idx);
                }
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
            // Keybind help, guarded like `:` so `?` still types into module edit fields.
            (None, _, KeyCode::Char('?'))
                if !self
                    .tabs
                    .get_mut(self.active)
                    .map(|t| t.view.is_overlay_active())
                    .unwrap_or(false) =>
            {
                self.help_open = true
            }
            (_, _, _) => {
                // A pending single-digit jump that never got a second digit still commits before
                // the key is forwarded to the tab.
                if let Some(KeyMode::TabDigit { first, .. }) = self.keymode {
                    self.switch_tab(first);
                }
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

    /// Jump straight to tab `idx`. Out-of-range indices are a silent no-op.
    pub(super) fn switch_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() && idx != self.active {
            self.set_content_focus(false);
            self.active = idx;
            self.set_content_focus(self.focus == Focus::Content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_digit_jumps_immediately_when_at_most_ten_tabs() {
        assert_eq!(digit_outcome(None, 3, 5), DigitOutcome::Jump(3));
    }

    #[test]
    fn first_digit_waits_when_it_could_start_a_two_digit_index() {
        assert_eq!(digit_outcome(None, 1, 25), DigitOutcome::Wait(1));
    }

    #[test]
    fn valid_two_digit_combo_jumps() {
        assert_eq!(digit_outcome(Some(1), 2, 25), DigitOutcome::Jump(12));
    }

    #[test]
    fn invalid_combo_falls_back_to_first_digit() {
        assert_eq!(digit_outcome(Some(1), 9, 15), DigitOutcome::Jump(1));
    }

    #[test]
    fn out_of_range_single_digit_is_ignored() {
        assert_eq!(digit_outcome(None, 7, 5), DigitOutcome::Ignore);
    }

    #[test]
    fn leading_zero_jumps_immediately_even_with_many_tabs() {
        assert_eq!(digit_outcome(None, 0, 25), DigitOutcome::Jump(0));
    }
}
