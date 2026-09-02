//! Extra-headers domain for the OCPP setup dialog (UI-R-059): a table of the client-only
//! `extra_headers` device config field, plus the add-inputs and edit/delete popups that mutate
//! it. Mirrors `crate::dialog::script_manager`'s table-cluster shape (`TableEntry`-derived row,
//! a `*Ref<'a>` borrow-splitting bundle so `#[derive(Focus)]`'s per-field `#[focus]` tags on
//! `OcppSetupDialog` keep working) and `crate::dialog::rename::RenamePrompt`'s popup shape,
//! widened from one field to two since editing a header needs both name and value.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, TableState, TableStateBuilder},
    style::{InputFieldStyleBuilder, TableStyleBuilder, TextStyle},
    traits::{HandleEvents, SetFocus},
    widgets::{InputField, InputFieldBuilder, Table, TableBuilder, Text, TextBuilder, Widget},
};
use ferrowl_ui_derive::TableEntry;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    style::Style,
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::view::border_style;

#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = HeaderHeader)]
pub(crate) struct HeaderRow {
    #[column(name = "Name", min = 10, max = 30)]
    name: String,
    #[column(name = "Value", min = 10, max = 40)]
    value: String,
}

pub(crate) type HeaderTable = Widget<TableState<HeaderRow, 2>, Table<HeaderRow, HeaderHeader, 2>>;

pub(crate) fn rows(headers: &[ferrowl_ocpp::HeaderDef]) -> Vec<HeaderRow> {
    headers
        .iter()
        .map(|h| HeaderRow {
            name: h.name.clone(),
            value: h.value.clone(),
        })
        .collect()
}

pub(crate) fn header_table(rows: Vec<HeaderRow>) -> HeaderTable {
    Widget {
        state: TableStateBuilder::default()
            .values(rows)
            .focused(false)
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Extra Headers".into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .row_margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(crate) fn header_name_input(border: Style) -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some("Header name".to_string()))
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Header Name", HorizontalAlignment::Left).into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border)
                    .build()
                    .expect("all required builder fields are set"),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(crate) fn header_value_input(border: Style) -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some("Header value".to_string()))
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Header Value", HorizontalAlignment::Left).into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border)
                    .build()
                    .expect("all required builder fields are set"),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

/// Selected row index, if any, bounds-checked against `headers` (the table's own selection can
/// briefly point past the end right after a delete, before the row refresh lands).
pub(crate) fn selected(headers: &[ferrowl_ocpp::HeaderDef], table: &HeaderTable) -> Option<usize> {
    let sel = table.state.table_state().selected()?;
    (sel < headers.len()).then_some(sel)
}

/// Bundle of `&mut` borrows into `OcppSetupDialog`'s own headers-cluster fields, so the shared
/// logic below can operate on them without the dialog owning a nested struct (which would break
/// `#[derive(Focus)]`'s per-field `#[focus]` tags — see the module doc comment).
pub(crate) struct HeaderTableRef<'a> {
    pub headers: &'a mut Vec<ferrowl_ocpp::HeaderDef>,
    pub table: &'a mut HeaderTable,
    pub name_input: &'a mut Widget<InputFieldState, InputField<String>>,
    pub value_input: &'a mut Widget<InputFieldState, InputField<String>>,
}

impl HeaderTableRef<'_> {
    pub fn selected(&self) -> Option<usize> {
        selected(self.headers, self.table)
    }

    pub fn refresh_rows(&mut self) {
        self.table.state.set_values(rows(self.headers));
    }

    /// Add a header from the two add-inputs (UI-R-059). On success the new header is pushed,
    /// rows refresh, the table selection moves to the new bottom row, and both inputs are
    /// cleared. On refusal (OC-R-117/118, via `HeaderDef::new`) both inputs are left untouched
    /// so correcting one field doesn't force retyping the other.
    pub fn add(&mut self) -> Result<(), ferrowl_ocpp::HeaderError> {
        let name = self.name_input.state.input().trim().to_string();
        let value = self.value_input.state.input().trim().to_string();
        let header = ferrowl_ocpp::HeaderDef::new(name, value)?;
        self.headers.push(header);
        self.refresh_rows();
        self.table.state.move_to_bottom();
        self.name_input.state.set_input(String::new());
        self.name_input.state.set_cursor(0);
        self.value_input.state.set_input(String::new());
        self.value_input.state.set_cursor(0);
        Ok(())
    }

    /// Open an edit prompt pre-filled from the selected row; `None` when no row is selected
    /// (UI-R-059's no-op-when-unselected rule).
    pub fn open_edit_prompt(&self) -> Option<HeaderEditPrompt> {
        let i = self.selected()?;
        let h = &self.headers[i];
        Some(HeaderEditPrompt::new(&h.name, &h.value))
    }

    /// Re-validate and replace the row at `index` in place (position preserved — UI-R-059 does
    /// not ask for re-sorting). Refuses (leaving the row untouched) on an invalid name/value.
    pub fn commit_edit(
        &mut self,
        index: usize,
        name: &str,
        value: &str,
    ) -> Result<(), ferrowl_ocpp::HeaderError> {
        let header = ferrowl_ocpp::HeaderDef::new(name, value)?;
        self.headers[index] = header;
        self.refresh_rows();
        Ok(())
    }

    /// Remove the selected row, if any, moving the selection up and refreshing (mirrors
    /// `ScriptManagerRef::delete_selected` exactly).
    pub fn delete_selected(&mut self) {
        if let Some(i) = self.selected() {
            self.headers.remove(i);
            self.refresh_rows();
            self.table.state.move_up();
        }
    }
}

// --- HeaderEditPrompt --------------------------------------------------------------------

/// Which field of a [`HeaderEditPrompt`] currently holds focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditField {
    Name,
    Value,
}

/// Outcome of feeding a key into a [`HeaderEditPrompt`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HeaderEditEvent {
    /// Key eaten, the prompt stays open.
    Consumed,
    /// `Esc`: drop the prompt, leave the header unchanged.
    Cancel,
    /// `Enter` on the value field: the host should try to apply this (trimmed) name/value pair.
    Commit(String, String),
}

/// Outcome of [`route_header_edit`]: what the host dialog should do about an open edit prompt.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HeaderEditOutcome {
    /// No prompt was open; the caller should route the key itself.
    NotActive,
    /// The prompt captured the key (cancelling itself if applicable).
    Consumed,
    /// The user confirmed a name/value pair. The prompt is **not** cleared: the host clears it
    /// only if the edit was accepted, so a refused pair leaves the prompt open (UI-R-059).
    Commit(String, String),
}

/// The header-edit popup: two pre-filled input fields (name, value) plus a keybind hint. No
/// existing shared two-field prompt fits (`RenamePrompt` is single-field).
#[derive(Clone, Debug)]
pub(crate) struct HeaderEditPrompt {
    name_input: Widget<InputFieldState, InputField<String>>,
    value_input: Widget<InputFieldState, InputField<String>>,
    active: EditField,
    keybinds: Widget<String, Text>,
}

impl HeaderEditPrompt {
    /// Open the prompt pre-filled with `name`/`value`; the name field starts focused.
    pub fn new(name: &str, value: &str) -> Self {
        let mut name_input = header_name_input(border_style());
        name_input.state.set_input(name.to_string());
        name_input.state.set_cursor(name.chars().count());
        name_input.state.set_focused(true);

        let mut value_input = header_value_input(border_style());
        value_input.state.set_input(value.to_string());
        value_input.state.set_cursor(value.chars().count());
        value_input.state.set_focused(false);

        Self {
            name_input,
            value_input,
            active: EditField::Name,
            keybinds: Widget {
                state: "<Tab>: switch | <Enter>: save | <Esc>: cancel".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(TextStyle::default())
                    .build()
                    .expect("all required builder fields are set"),
            },
        }
    }

    #[cfg(test)]
    pub fn name_input(&self) -> &Widget<InputFieldState, InputField<String>> {
        &self.name_input
    }

    #[cfg(test)]
    pub fn value_input(&self) -> &Widget<InputFieldState, InputField<String>> {
        &self.value_input
    }

    fn switch_active(&mut self) {
        self.active = match self.active {
            EditField::Name => EditField::Value,
            EditField::Value => EditField::Name,
        };
        self.name_input
            .state
            .set_focused(self.active == EditField::Name);
        self.value_input
            .state
            .set_focused(self.active == EditField::Value);
    }

    /// Feed one key while the prompt is open.
    pub fn handle_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> HeaderEditEvent {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => HeaderEditEvent::Cancel,
            (KeyModifiers::NONE, KeyCode::Tab)
            | (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.switch_active();
                HeaderEditEvent::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) => match self.active {
                EditField::Value => HeaderEditEvent::Commit(
                    self.name_input.state.input().trim().to_string(),
                    self.value_input.state.input().trim().to_string(),
                ),
                EditField::Name => {
                    self.switch_active();
                    HeaderEditEvent::Consumed
                }
            },
            _ => {
                let field = match self.active {
                    EditField::Name => &mut self.name_input,
                    EditField::Value => &mut self.value_input,
                };
                let _ = field.state.handle_events(modifiers, code);
                HeaderEditEvent::Consumed
            }
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let [_, hc, _] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(46),
            Constraint::Min(1),
        ])
        .areas(area);
        // 2 border + 2 margin + 3 name + 3 value + 1 keybinds = 11
        let [_, popup, _] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(11),
            Constraint::Min(1),
        ])
        .areas(hc);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title(" Edit Header ");
        let inner = block.inner(popup).inner(Margin::new(2, 1));
        UiWidget::render(&Clear, popup, buf);
        block.render(popup, buf);

        let [name_area, value_area, keybinds_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.name_input.widget,
            name_area,
            buf,
            &mut self.name_input.state,
        );
        StatefulWidget::render(
            &self.value_input.widget,
            value_area,
            buf,
            &mut self.value_input.state,
        );
        StatefulWidget::render(
            &self.keybinds.widget,
            keybinds_area,
            buf,
            &mut self.keybinds.state,
        );
    }
}

/// Feed one key through `prompt`, if the header-edit prompt is open. Clears `*prompt` on cancel;
/// on commit the prompt is left in place for the host to keep (refused) or clear (accepted).
pub(crate) fn route_header_edit(
    prompt: &mut Option<HeaderEditPrompt>,
    modifiers: KeyModifiers,
    code: KeyCode,
) -> HeaderEditOutcome {
    let Some(p) = prompt.as_mut() else {
        return HeaderEditOutcome::NotActive;
    };
    match p.handle_key(modifiers, code) {
        HeaderEditEvent::Consumed => HeaderEditOutcome::Consumed,
        HeaderEditEvent::Cancel => {
            *prompt = None;
            HeaderEditOutcome::Consumed
        }
        HeaderEditEvent::Commit(name, value) => HeaderEditOutcome::Commit(name, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, value: &str) -> ferrowl_ocpp::HeaderDef {
        ferrowl_ocpp::HeaderDef::new(name, value).unwrap()
    }

    #[test]
    /// OC-R-117 — `rows()` preserves the configured headers' insertion order (not sorted).
    fn ut_header_row_rows_preserve_insertion_order() {
        let headers = vec![
            header("X-Tenant", "acme-1"),
            header("X-A", "1"),
            header("X-B", "2"),
        ];
        let got = rows(&headers);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].name, "X-Tenant");
        assert_eq!(got[1].name, "X-A");
        assert_eq!(got[2].name, "X-B");
    }

    fn table_ref_fixture<'a>(
        headers: &'a mut Vec<ferrowl_ocpp::HeaderDef>,
        table: &'a mut HeaderTable,
        name_input: &'a mut Widget<InputFieldState, InputField<String>>,
        value_input: &'a mut Widget<InputFieldState, InputField<String>>,
    ) -> HeaderTableRef<'a> {
        HeaderTableRef {
            headers,
            table,
            name_input,
            value_input,
        }
    }

    #[test]
    /// OC-R-117 — adding a header with a reserved name is refused; the add-inputs keep their
    /// text so correcting one field doesn't force retyping the other.
    fn ut_header_add_rejects_reserved_name() {
        let mut headers = Vec::new();
        let mut table = header_table(rows(&headers));
        let mut name_input = header_name_input(Style::default());
        let mut value_input = header_value_input(Style::default());
        let mut r = table_ref_fixture(&mut headers, &mut table, &mut name_input, &mut value_input);

        r.name_input.state.set_input("Authorization".to_string());
        r.value_input.state.set_input("Basic xyz".to_string());
        assert!(r.add().is_err());
        assert!(r.headers.is_empty());
        assert_eq!(r.name_input.state.input(), "Authorization");
        assert_eq!(r.value_input.state.input(), "Basic xyz");
    }

    #[test]
    /// OC-R-118 — a valid add succeeds and clears both inputs.
    fn ut_header_add_accepts_and_clears_inputs() {
        let mut headers = Vec::new();
        let mut table = header_table(rows(&headers));
        let mut name_input = header_name_input(Style::default());
        let mut value_input = header_value_input(Style::default());
        let mut r = table_ref_fixture(&mut headers, &mut table, &mut name_input, &mut value_input);

        r.name_input.state.set_input("X-Tenant".to_string());
        r.value_input.state.set_input("acme-1".to_string());
        assert!(r.add().is_ok());
        assert_eq!(r.headers.len(), 1);
        assert_eq!(r.headers[0].name, "X-Tenant");
        assert_eq!(r.name_input.state.input(), "");
        assert_eq!(r.value_input.state.input(), "");
    }

    #[test]
    /// UI-R-096 — opening the edit prompt with no row selected is a no-op.
    fn ut_open_edit_prompt_none_when_unselected() {
        let mut headers = Vec::new();
        let mut table = header_table(rows(&headers));
        let mut name_input = header_name_input(Style::default());
        let mut value_input = header_value_input(Style::default());
        let r = table_ref_fixture(&mut headers, &mut table, &mut name_input, &mut value_input);
        assert!(r.open_edit_prompt().is_none());
    }

    #[test]
    /// OC-R-117 — an accepted edit replaces the row in place; a refused edit (reserved name)
    /// leaves the row untouched.
    fn ut_commit_edit_replaces_in_place_or_refuses() {
        let mut headers = vec![header("X-A", "1"), header("X-B", "2")];
        let mut table = header_table(rows(&headers));
        let mut name_input = header_name_input(Style::default());
        let mut value_input = header_value_input(Style::default());
        let mut r = table_ref_fixture(&mut headers, &mut table, &mut name_input, &mut value_input);

        assert!(r.commit_edit(0, "X-A2", "1b").is_ok());
        assert_eq!(r.headers[0].name, "X-A2");
        assert_eq!(r.headers[1].name, "X-B");

        assert!(r.commit_edit(1, "Authorization", "x").is_err());
        assert_eq!(r.headers[1].name, "X-B");
    }

    #[test]
    /// UI-R-097 — deleting the selected row removes it and moves the selection up.
    fn ut_delete_selected_removes_row() {
        let mut headers = vec![header("X-A", "1")];
        let mut table = header_table(rows(&headers));
        let mut name_input = header_name_input(Style::default());
        let mut value_input = header_value_input(Style::default());
        let mut r = table_ref_fixture(&mut headers, &mut table, &mut name_input, &mut value_input);
        r.delete_selected();
        assert!(r.headers.is_empty());
    }

    #[test]
    /// UI-R-059 — `Tab`/`BackTab` toggles which field of the edit prompt is active.
    fn ut_header_edit_prompt_tab_switches_field() {
        let mut prompt = HeaderEditPrompt::new("X-A", "1");
        assert_eq!(prompt.active, EditField::Name);
        assert_eq!(
            prompt.handle_key(KeyModifiers::NONE, KeyCode::Tab),
            HeaderEditEvent::Consumed
        );
        assert_eq!(prompt.active, EditField::Value);
        assert_eq!(
            prompt.handle_key(KeyModifiers::NONE, KeyCode::BackTab),
            HeaderEditEvent::Consumed
        );
        assert_eq!(prompt.active, EditField::Name);
    }

    #[test]
    /// UI-R-096 — `Esc` cancels the edit prompt without applying any change.
    fn ut_header_edit_prompt_esc_cancels() {
        let mut prompt = HeaderEditPrompt::new("X-A", "1");
        assert_eq!(
            prompt.handle_key(KeyModifiers::NONE, KeyCode::Esc),
            HeaderEditEvent::Cancel
        );
    }

    #[test]
    /// UI-R-096 — `Enter` on the value field commits the trimmed name/value pair.
    fn ut_header_edit_prompt_enter_on_value_commits() {
        let mut prompt = HeaderEditPrompt::new("X-A", "1");
        prompt.switch_active(); // move to the value field
        assert_eq!(
            prompt.handle_key(KeyModifiers::NONE, KeyCode::Enter),
            HeaderEditEvent::Commit("X-A".to_string(), "1".to_string())
        );
    }

    #[test]
    /// UI-R-059 — `Esc` clears an open prompt; a commit leaves it in place for the host to judge.
    fn ut_route_header_edit_cancel_clears_commit_keeps() {
        let mut prompt = Some(HeaderEditPrompt::new("X-A", "1"));
        prompt.as_mut().unwrap().switch_active();
        assert_eq!(
            route_header_edit(&mut prompt, KeyModifiers::NONE, KeyCode::Enter),
            HeaderEditOutcome::Commit("X-A".to_string(), "1".to_string())
        );
        assert!(
            prompt.is_some(),
            "a commit must not clear the prompt itself"
        );

        assert_eq!(
            route_header_edit(&mut prompt, KeyModifiers::NONE, KeyCode::Esc),
            HeaderEditOutcome::Consumed
        );
        assert!(prompt.is_none());
    }

    #[test]
    /// UI-R-059 — routing reports NotActive when no edit prompt is open.
    fn ut_route_header_edit_not_active_when_none() {
        let mut prompt: Option<HeaderEditPrompt> = None;
        assert_eq!(
            route_header_edit(&mut prompt, KeyModifiers::NONE, KeyCode::Enter),
            HeaderEditOutcome::NotActive
        );
    }

    #[test]
    fn ut_edit_prompt_render_does_not_panic() {
        let mut prompt = HeaderEditPrompt::new("X-A", "1");
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        prompt.render(area, &mut buf);
    }
}
