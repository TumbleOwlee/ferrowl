//! Session-level dialog (`:session`): the session's Lua scripts (reusing [`ScriptDialog`] as an
//! embedded pane), a sim-cycle interval field, and a read-only tail of the session sim's log.
//!
//! Focus is two-level: [`SessionDialogFocus::Interval`] and [`SessionDialogFocus::Scripts`].
//! `Tab`/`Shift+Tab` on the interval field move into the scripts pane; inside the scripts pane
//! `Tab`/`Shift+Tab` cycle its own fields (table/name input/code editor) exactly as they do in a
//! standalone [`ScriptDialog`], and `Esc` there steps back out to the interval field rather than
//! closing the whole dialog. `Esc` on the interval field closes the dialog, applying the working
//! copy — the same "no separate save, edits are live" convention [`ScriptDialog`] itself uses.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder},
    style::InputFieldStyleBuilder,
    traits::HandleEvents,
    widgets::{InputField, InputFieldBuilder, Validate, ValidateResult, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::config::script::ScriptDef;
use crate::dialog::scripts::ScriptDialog;
use crate::module::view::SharedLog;
use crate::view::log::{LogEntry, LogView, format_timestamp, new_log_view};

/// Sim-cycle interval input validator: must parse as a finite, positive number of seconds
/// (mirrors `Session::interval_duration`'s sanitization, but rejected at the field instead of
/// silently falling back).
#[derive(Clone, Debug)]
pub struct Interval();

impl Validate for Interval {
    fn validate(input: &str) -> ValidateResult {
        match input.trim().parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => ValidateResult::None,
            Ok(_) => ValidateResult::Error("Interval must be a positive number".to_string()),
            Err(e) => ValidateResult::Error(e.to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionDialogFocus {
    Interval,
    Scripts,
}

/// The `:session` dialog. Works on a private copy of the scripts/interval; the caller applies the
/// result via [`SessionDialog::resolve`] on close.
pub struct SessionDialog {
    interval: Widget<InputFieldState, InputField<Interval>>,
    scripts: ScriptDialog,
    log: LogView,
    focus: SessionDialogFocus,
}

impl SessionDialog {
    pub fn new(scripts: &[ScriptDef], interval: Duration) -> Self {
        let mut interval_field = interval_input();
        set_input(&mut interval_field, &format_interval(interval));
        let mut scripts = ScriptDialog::new(scripts);
        scripts.set_embedded_focused(false);
        let mut dialog = Self {
            interval: interval_field,
            scripts,
            log: new_log_view(),
            focus: SessionDialogFocus::Interval,
        };
        dialog.interval.state.set_focused(true);
        dialog
    }

    /// Apply the working copy back to the caller: the validated interval (falling back to the
    /// previous value if the field is currently invalid — the caller only reaches here via the
    /// Esc-from-Interval close path, but an invalid field must never propagate a bogus duration)
    /// and the scripts list.
    pub fn resolve(self) -> (Vec<ScriptDef>, Duration) {
        let interval = parse_interval(self.interval.state.input())
            .unwrap_or_else(|| Duration::from_secs_f64(1.0));
        (self.scripts.resolve(), interval)
    }

    /// Refresh the read-only log pane from a snapshot of the session sim's log ring. Called by
    /// the owner once per tick while the dialog is open.
    pub fn set_log_entries(&mut self, entries: Vec<LogEntry>) {
        self.log.state.set_values(entries);
        self.log.state.move_to_bottom();
    }

    fn move_to_scripts(&mut self) {
        self.focus = SessionDialogFocus::Scripts;
        self.interval.state.set_focused(false);
        self.scripts.set_embedded_focused(true);
    }

    fn move_to_interval(&mut self) {
        self.focus = SessionDialogFocus::Interval;
        self.scripts.set_embedded_focused(false);
        self.interval.state.set_focused(true);
    }

    /// Handle a key. Returns `true` when the dialog should close (Esc on the interval field).
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        match self.focus {
            SessionDialogFocus::Interval => match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => return true,
                (KeyModifiers::NONE, KeyCode::Tab)
                | (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    self.move_to_scripts()
                }
                _ => {
                    let _ = self.interval.state.handle_events(modifiers, code);
                }
            },
            SessionDialogFocus::Scripts => {
                if self.scripts.handle_events(modifiers, code) {
                    // The embedded dialog's own Esc-at-top-level: step back out to the interval
                    // field instead of closing the whole session dialog.
                    self.move_to_interval();
                }
            }
        }
        false
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let [_, hc, _] = Layout::horizontal([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(area);
        let [_, vc, _] = Layout::vertical([
            Constraint::Percentage(5),
            Constraint::Percentage(90),
            Constraint::Percentage(5),
        ])
        .areas(hc);

        UiWidget::render(&Clear, vc, buf);
        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("Session");
        let inner = block.inner(vc);
        block.render(vc, buf);
        let inner = inner.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let [interval_area, scripts_area, log_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(10),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.interval.widget,
            interval_area,
            buf,
            &mut self.interval.state,
        );
        self.scripts.render(scripts_area, buf);
        StatefulWidget::render(&self.log.widget, log_area, buf, &mut self.log.state);
    }
}

fn parse_interval(input: &str) -> Option<Duration> {
    let v = input.trim().parse::<f64>().ok()?;
    (v.is_finite() && v > 0.0).then(|| Duration::from_secs_f64(v))
}

fn format_interval(interval: Duration) -> String {
    let secs = interval.as_secs_f64();
    // Trim a trailing ".0" so a whole-second interval reads as "1", not "1.0000..".
    let text = format!("{secs:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn set_input(widget: &mut Widget<InputFieldState, InputField<Interval>>, value: &str) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

fn interval_input() -> Widget<InputFieldState, InputField<Interval>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some("1.0".to_string()))
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(
                ("Interval (seconds)", HorizontalAlignment::Left).into(),
            ))
            .style(InputFieldStyleBuilder::default().build().unwrap())
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

/// Build `LogEntry` rows from a raw `(timestamp_ms, message)` ring snapshot, matching the
/// formatting `App::refresh_snapshot` applies to tab logs.
pub fn entries_from_ring(lines: Vec<(u64, String)>) -> Vec<LogEntry> {
    lines
        .into_iter()
        .map(|(ts, msg)| LogEntry {
            timestamp: format_timestamp(ts),
            message: msg.trim_end_matches('\u{0}').to_string(),
        })
        .collect()
}

/// Snapshot `log`'s current lines as render-ready entries. Async because the log is behind a
/// `tokio::sync::RwLock`.
pub async fn snapshot_log(log: &SharedLog, max: usize) -> Vec<LogEntry> {
    let lines = log.read().await.peek_n(max);
    entries_from_ring(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> SessionDialog {
        SessionDialog::new(
            &[ScriptDef {
                name: "boot".into(),
                code: String::new(),
                enabled: true,
            }],
            Duration::from_secs_f64(2.5),
        )
    }

    #[test]
    fn ut_new_prefills_interval_and_scripts() {
        let d = dialog();
        assert_eq!(d.interval.state.input(), "2.5");
        assert_eq!(d.scripts.resolve().len(), 1);
    }

    #[test]
    fn ut_interval_validate_accepts_positive_finite() {
        assert!(matches!(Interval::validate("2.5"), ValidateResult::None));
        assert!(matches!(Interval::validate("1"), ValidateResult::None));
    }

    #[test]
    fn ut_interval_validate_rejects_bad_values() {
        assert!(matches!(Interval::validate("0"), ValidateResult::Error(_)));
        assert!(matches!(Interval::validate("-1"), ValidateResult::Error(_)));
        assert!(matches!(
            Interval::validate("nan"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(
            Interval::validate("inf"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(
            Interval::validate("abc"),
            ValidateResult::Error(_)
        ));
    }

    #[test]
    fn ut_resolve_round_trips_scripts_and_interval() {
        let d = dialog();
        let (scripts, interval) = d.resolve();
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].name, "boot");
        assert_eq!(interval, Duration::from_secs_f64(2.5));
    }

    #[test]
    fn ut_resolve_falls_back_on_invalid_interval() {
        let mut d = dialog();
        set_input(&mut d.interval, "not-a-number");
        let (_, interval) = d.resolve();
        assert_eq!(interval, Duration::from_secs_f64(1.0));
    }

    #[test]
    fn ut_tab_from_interval_moves_to_scripts_and_back_on_esc() {
        let mut d = dialog();
        assert_eq!(d.focus, SessionDialogFocus::Interval);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Tab));
        assert_eq!(d.focus, SessionDialogFocus::Scripts);
        // Esc while the scripts pane's own focus sits on its Table (top level) steps back out
        // instead of closing the whole dialog.
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert_eq!(d.focus, SessionDialogFocus::Interval);
    }

    #[test]
    fn ut_esc_from_interval_closes() {
        let mut d = dialog();
        assert!(d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
    }

    #[test]
    fn ut_typing_in_interval_updates_input() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('9'));
        assert!(d.interval.state.input().contains('9'));
    }

    #[test]
    fn ut_entries_from_ring_trims_nul_padding() {
        let entries = entries_from_ring(vec![(0, "hello\u{0}\u{0}".to_string())]);
        assert_eq!(entries[0].message, "hello");
    }

    #[test]
    fn ut_format_interval_trims_trailing_zeros() {
        assert_eq!(format_interval(Duration::from_secs_f64(1.0)), "1");
        assert_eq!(format_interval(Duration::from_secs_f64(0.25)), "0.25");
    }
}
