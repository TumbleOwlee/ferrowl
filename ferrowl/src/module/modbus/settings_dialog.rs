//! Module settings dialog: edits the read ranges of the tab's device config.

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult, render_field, render_row,
    state::InputFieldState,
    style::{InputFieldStyle, TextStyle},
    traits::HandleEvents,
    widgets::{InputField, Text, TextBuilder, Validate, ValidateResult, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, Widget as UiWidget},
};

use crate::config::device::{ReadRanges, check_range_spec};
use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmOutcome, route_close_confirm};
use crate::dialog::widgets::{input, set_input};

/// Validation marker for a read-range input: the strict `check_range_spec` check.
#[derive(Debug, Clone)]
pub struct RangeSpec;

impl Validate for RangeSpec {
    fn validate(input: &str) -> ValidateResult {
        match check_range_spec(input) {
            Ok(()) => ValidateResult::None,
            Err(m) => ValidateResult::Error(m),
        }
    }
}

#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct ModuleSettingsDialog {
    #[focus]
    pub holding: Widget<InputFieldState, InputField<RangeSpec>>,
    #[focus]
    pub input: Widget<InputFieldState, InputField<RangeSpec>>,
    #[focus]
    pub coils: Widget<InputFieldState, InputField<RangeSpec>>,
    #[focus]
    pub discrete: Widget<InputFieldState, InputField<RangeSpec>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
    /// Confirm-close popup, opened with Esc.
    #[builder(default)]
    pub close_confirm: Option<CloseConfirmDialog>,
    /// Set once the close-confirm popup is confirmed; the host closes the dialog via
    /// `take_close_request`.
    #[builder(default)]
    close_requested: bool,
}

impl ModuleSettingsDialog {
    pub fn new(ranges: &ReadRanges) -> Self {
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::style::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let mut d = ModuleSettingsDialogBuilder::default()
            .holding(input("Holding ranges", "0-100,140-160", &input_style, true))
            .input(input("Input ranges", "0-9", &input_style, false))
            .coils(input("Coil ranges", "0-31", &input_style, false))
            .discrete(input("Discrete ranges", "0-31", &input_style, false))
            .error(Widget {
                state: String::new(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .multiline(true)
                    .style(error_style)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .keybinds(Widget {
                state: "<Tab> next   <Enter> confirm   <Esc> cancel".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(TextStyle::default())
                    .build()
                    .expect("all required builder fields are set"),
            })
            .focus(ModuleSettingsDialogFocus::Holding)
            .build()
            .expect("all required builder fields are set");
        for (field, value) in [
            (&mut d.holding, &ranges.holding),
            (&mut d.input, &ranges.input),
            (&mut d.coils, &ranges.coils),
            (&mut d.discrete, &ranges.discrete),
        ] {
            if let Some(v) = value {
                set_input(field, v);
            }
        }
        d
    }

    /// Route a key: the close-confirm popup captures all keys while open; `Esc` opens it;
    /// everything else falls through to the derived per-field routing.
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match route_close_confirm(&mut self.close_confirm, modifiers, code) {
            CloseConfirmOutcome::NotActive => {}
            CloseConfirmOutcome::Close => {
                self.close_requested = true;
                return EventResult::Consumed;
            }
            CloseConfirmOutcome::Consumed => return EventResult::Consumed,
        }

        if modifiers == KeyModifiers::NONE && code == KeyCode::Esc {
            self.close_confirm = Some(CloseConfirmDialog::new());
            return EventResult::Consumed;
        }

        <Self as HandleEvents>::handle_events(self, modifiers, code)
    }

    /// Whether the close-confirm popup was confirmed since the last call; clears the flag.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// The first failing input, as `<field title>: <message>`, else the trimmed texts with
    /// blank inputs left unset.
    pub fn resolve(&self) -> Result<ReadRanges, String> {
        for (title, field) in [
            ("Holding ranges", &self.holding),
            ("Input ranges", &self.input),
            ("Coil ranges", &self.coils),
            ("Discrete ranges", &self.discrete),
        ] {
            if let Err(m) = check_range_spec(field.state.input()) {
                return Err(format!("{title}: {m}"));
            }
        }
        let text = |field: &Widget<InputFieldState, InputField<RangeSpec>>| {
            let t = field.state.input().trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        Ok(ReadRanges {
            holding: text(&self.holding),
            input: text(&self.input),
            coils: text(&self.coils),
            discrete: text(&self.discrete),
        })
    }

    /// The resolved ranges, or `None` while any input fails its check (MB-R-292); the error box
    /// is kept current by `render`.
    pub fn confirm(&mut self) -> Option<ReadRanges> {
        self.refresh_error();
        self.resolve().ok()
    }

    /// Mirrors the first failing input into the error box, clearing it once all pass (MB-R-301).
    fn refresh_error(&mut self) {
        match self.resolve() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.refresh_error();
        // border(2) + inner margin(2) + two field rows(6) + error(4) + keybinds(1)
        let box_height = 15;
        let [_, hcenter, _] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(60),
            Constraint::Min(1),
        ])
        .areas(area);
        let [_, vcenter, _] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(box_height),
            Constraint::Min(1),
        ])
        .areas(hcenter);

        Clear.render(vcenter, buf);
        let block = Block::bordered()
            .style(
                ratatui::style::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("Module Settings");
        let inner = block.inner(vcenter).inner(Margin::new(2, 1));
        block.render(vcenter, buf);

        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(inner);
        render_row!(self, rows[0], buf; holding, input);
        render_row!(self, rows[1], buf; coils, discrete);
        if !self.error.state.is_empty() {
            render_field!(self, error, rows[2], buf);
        }
        render_field!(self, keybinds, rows[3], buf);

        if let Some(d) = self.close_confirm.as_mut() {
            d.render(vcenter, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn rendered(d: &mut ModuleSettingsDialog) -> Buffer {
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        buf
    }

    fn empty() -> ModuleSettingsDialog {
        ModuleSettingsDialog::new(&ReadRanges::default())
    }

    #[test]
    /// MB-R-289 — the dialog opens with each input holding the device's current range text.
    fn ut_new_prefills_from_read_ranges() {
        let d = ModuleSettingsDialog::new(&ReadRanges {
            holding: Some("0-10".into()),
            discrete: Some("3".into()),
            ..Default::default()
        });
        assert_eq!(d.holding.state.input(), "0-10");
        assert_eq!(d.discrete.state.input(), "3");
        assert_eq!(d.input.state.input(), "");
        assert_eq!(d.coils.state.input(), "");
    }

    #[test]
    /// MB-R-288 — the dialog offers one input per register kind.
    fn ut_render_shows_four_range_inputs() {
        let mut d = empty();
        let text = buffer_text(&rendered(&mut d));
        for title in [
            "Holding ranges",
            "Input ranges",
            "Coil ranges",
            "Discrete ranges",
        ] {
            assert!(text.contains(title), "missing {title}");
        }
    }

    #[test]
    /// MB-R-291 — a malformed or reversed range paints its input's border as an error.
    fn ut_malformed_input_paints_error_border() {
        let style = InputFieldStyle::default();
        let corner = |buf: &Buffer| -> (u16, u16) {
            let mut corners = Vec::new();
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    if buf[(x, y)].symbol() == "┌" {
                        corners.push((x, y));
                    }
                }
            }
            corners.sort_by_key(|(x, y)| (*y, *x));
            corners[1]
        };
        let mut d = empty();
        set_input(&mut d.holding, "10-5");
        let buf = rendered(&mut d);
        assert_eq!(buf[corner(&buf)].fg, style.error().fg.unwrap());
        set_input(&mut d.holding, "0-10");
        let buf = rendered(&mut d);
        assert_eq!(buf[corner(&buf)].fg, style.focused().fg.unwrap());
    }

    #[test]
    /// MB-R-292, MB-R-301 — a refused confirm names the first failing field in the error box.
    fn ut_confirm_refused_names_first_failing_field() {
        let mut d = empty();
        set_input(&mut d.holding, "0-5");
        set_input(&mut d.input, "a");
        set_input(&mut d.coils, "x");
        assert_eq!(d.confirm(), None);
        assert_eq!(
            d.error.state,
            "Input ranges: 'a' is not a range (use 5 or 0-100, start ≤ end)"
        );
        let text = buffer_text(&rendered(&mut d));
        assert!(text.contains("Error"));
        assert!(text.contains("'a'"));
    }

    #[test]
    /// MB-R-301 — the error box follows the inputs live, appearing and clearing on render without a confirm.
    fn ut_error_box_tracks_inputs_live() {
        let mut d = empty();
        rendered(&mut d);
        assert!(d.error.state.is_empty());
        set_input(&mut d.holding, "x");
        let text = buffer_text(&rendered(&mut d));
        assert!(
            d.error.state.contains("Holding ranges"),
            "{}",
            d.error.state
        );
        assert!(text.contains("Error"));
        set_input(&mut d.holding, "0-5");
        let text = buffer_text(&rendered(&mut d));
        assert!(d.error.state.is_empty());
        assert!(!text.contains("Error"));
    }

    #[test]
    /// MB-R-293 — confirm yields the trimmed input texts, a blank input staying unset.
    fn ut_confirm_maps_text_and_blank_to_unset() {
        let mut d = empty();
        set_input(&mut d.holding, " 0-10 ");
        set_input(&mut d.coils, "   ");
        assert_eq!(
            d.confirm(),
            Some(ReadRanges {
                holding: Some("0-10".into()),
                input: None,
                coils: None,
                discrete: None,
            })
        );
    }

    #[test]
    /// MB-R-294 — Esc opens the close-confirm popup; confirming it requests the close.
    fn ut_esc_opens_close_confirm_then_enter_requests_close() {
        let mut d = empty();
        set_input(&mut d.holding, "x");
        let r = d.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(matches!(r, EventResult::Consumed));
        assert!(d.close_confirm.is_some());
        assert!(!d.take_close_request());
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(d.take_close_request());
    }
}
