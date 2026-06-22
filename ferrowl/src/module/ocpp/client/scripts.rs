//! Lua script manager dialog, shared by both OCPP client views. Left: a table of scripts with an
//! On/Off status over a "New Script" name input; right: a code editor for the selected script.
//! Edits are live on a working copy; the view reads it back via [`ScriptDialog::resolve`] on close
//! and reloads the simulation. `t` toggles a script, `d` deletes (with confirmation), `c` toggles
//! compact rows, Enter in the name input creates a new (enabled) script, Esc closes.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{
        CodeInputFieldState, CodeInputFieldStateBuilder, InputFieldState, InputFieldStateBuilder,
        TableState, TableStateBuilder,
    },
    style::{InputFieldStyleBuilder, TableStyleBuilder},
    traits::HandleEvents,
    widgets::{
        CodeInputField, CodeInputFieldBuilder, Header, InputField, InputFieldBuilder, Table,
        TableBuilder, TableEntry, Widget, Width,
    },
};
use ratatui::style::Style;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::module::modbus::dialog::ConfirmDeleteDialog;
use crate::module::ocpp::config::device::ScriptDef;

// --- Script table ----------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct ScriptRow {
    name: String,
    status: String,
}

impl TableEntry<2> for ScriptRow {
    fn values(&self) -> [String; 2] {
        [self.name.clone(), self.status.clone()]
    }
    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone, Debug)]
struct ScriptHeader;

impl Header<2> for ScriptHeader {
    fn header() -> [String; 2] {
        ["Name".into(), "Status".into()]
    }
    fn widths() -> [Width; 2] {
        [Width { min: 10, max: 40 }, Width { min: 6, max: 6 }]
    }
}

type ScriptTable = Widget<TableState<ScriptRow, 2>, Table<ScriptRow, ScriptHeader, 2>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    NewInput,
    Code,
}

/// The script manager dialog. Works on a private copy of the script list; the view applies the
/// result on close.
pub struct ScriptDialog {
    scripts: Vec<ScriptDef>,
    table: ScriptTable,
    name_input: Widget<InputFieldState, InputField<String>>,
    code: Widget<CodeInputFieldState, CodeInputField>,
    focus: Focus,
    confirm: Option<ConfirmDeleteDialog>,
    /// Compact (no vertical row margin) script table; toggled with `c`. Default off (margin 1).
    compact: bool,
}

impl ScriptDialog {
    pub fn new(scripts: &[ScriptDef]) -> Self {
        let scripts = scripts.to_vec();
        let mut dialog = Self {
            table: script_table(rows(&scripts)),
            name_input: name_input(),
            code: code_editor(),
            focus: Focus::List,
            confirm: None,
            compact: false,
            scripts,
        };
        dialog.sync_code_from_selection();
        dialog
    }

    /// Apply the working copy back to the caller. Flushes the open editor into the selected script
    /// first so unsaved keystrokes aren't lost.
    pub fn resolve(mut self) -> Vec<ScriptDef> {
        self.flush_code_to_selection();
        self.scripts
    }

    fn selected(&self) -> Option<usize> {
        let sel = self.table.state.table_state().selected()?;
        (sel < self.scripts.len()).then_some(sel)
    }

    /// Load the selected script's code into the editor.
    fn sync_code_from_selection(&mut self) {
        let content = self
            .selected()
            .map(|i| self.scripts[i].code.clone())
            .unwrap_or_default();
        self.code.state.set_content(&content);
    }

    /// Write the editor's content back into the selected script.
    fn flush_code_to_selection(&mut self) {
        if let Some(i) = self.selected() {
            self.scripts[i].code = self.code.state.content();
        }
    }

    fn refresh_rows(&mut self) {
        self.table.state.set_values(rows(&self.scripts));
    }

    /// Create a new enabled script from the name input (rejecting empty / duplicate names).
    fn create_script(&mut self) {
        let name = self.name_input.state.input().trim().to_string();
        if name.is_empty() || self.scripts.iter().any(|s| s.name == name) {
            return;
        }
        self.scripts.push(ScriptDef {
            name,
            code: String::new(),
            enabled: true,
        });
        self.refresh_rows();
        self.table.state.move_to_bottom();
        self.name_input.state.set_input(String::new());
        self.name_input.state.set_cursor(0);
        self.sync_code_from_selection();
    }

    fn toggle_compact(&mut self) {
        self.compact = !self.compact;
        self.table.widget.set_row_margin(Margin {
            vertical: if self.compact { 0 } else { 1 },
            horizontal: 0,
        });
    }

    fn toggle_selected(&mut self) {
        if let Some(i) = self.selected() {
            self.scripts[i].enabled = !self.scripts[i].enabled;
            self.refresh_rows();
        }
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected() {
            self.scripts.remove(i);
            self.refresh_rows();
            self.table.state.move_up();
            self.sync_code_from_selection();
        }
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::List => Focus::NewInput,
            Focus::NewInput => Focus::Code,
            Focus::Code => Focus::List,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Focus::List => Focus::Code,
            Focus::NewInput => Focus::List,
            Focus::Code => Focus::NewInput,
        };
    }

    /// Handle a key. Returns `true` when the dialog should close (Esc at the top level).
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        // Delete-confirmation sub-dialog takes precedence.
        if let Some(confirm) = self.confirm.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.confirm = None,
                (KeyModifiers::NONE, KeyCode::Tab) => confirm.focus_next(),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    confirm.focus_previous()
                }
                (KeyModifiers::NONE, KeyCode::Enter | KeyCode::Char(' ')) => {
                    let confirmed = confirm.is_confirm_focused();
                    self.confirm = None;
                    if confirmed {
                        self.delete_selected();
                    }
                }
                _ => {
                    let _ = confirm.handle_events(modifiers, code);
                }
            }
            return false;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => return true,
            (KeyModifiers::NONE, KeyCode::Tab) => self.focus_next(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => self.focus_previous(),
            _ => match self.focus {
                Focus::List => match (modifiers, code) {
                    (KeyModifiers::NONE, KeyCode::Char('t')) => self.toggle_selected(),
                    (KeyModifiers::NONE, KeyCode::Char('c')) => self.toggle_compact(),
                    (KeyModifiers::NONE, KeyCode::Char('d')) => {
                        if let Some(i) = self.selected() {
                            self.confirm = Some(ConfirmDeleteDialog::new(&self.scripts[i].name));
                        }
                    }
                    _ => {
                        let _ = self.table.state.handle_events(modifiers, code);
                        self.sync_code_from_selection();
                    }
                },
                Focus::NewInput => match (modifiers, code) {
                    (KeyModifiers::NONE, KeyCode::Enter) => self.create_script(),
                    _ => {
                        let _ = self.name_input.state.handle_events(modifiers, code);
                    }
                },
                Focus::Code => {
                    let _ = self.code.state.handle_events(modifiers, code);
                    self.flush_code_to_selection();
                }
            },
        }
        false
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Centered box covering most of the screen.
        let [_, hc, _] = Layout::horizontal([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(area);
        let [_, vc, _] = Layout::vertical([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
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
            .title("Lua Scripts");
        let inner = block.inner(vc);
        block.render(vc, buf);
        let inner = inner.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Min(1)]).areas(inner);
        let [list_area, input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(left);

        self.table.state.set_focused(self.focus == Focus::List);
        self.name_input
            .state
            .set_focused(self.focus == Focus::NewInput);
        self.code.state.set_focused(self.focus == Focus::Code);

        StatefulWidget::render(&self.table.widget, list_area, buf, &mut self.table.state);
        StatefulWidget::render(
            &self.name_input.widget,
            input_area,
            buf,
            &mut self.name_input.state,
        );
        StatefulWidget::render(&self.code.widget, right, buf, &mut self.code.state);

        if let Some(confirm) = self.confirm.as_mut() {
            confirm.render(area, buf);
        }
    }
}

/// Theme border color for unfocused fields, matching the table/selection borders.
fn border_style() -> Style {
    Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)
}

fn rows(scripts: &[ScriptDef]) -> Vec<ScriptRow> {
    scripts
        .iter()
        .map(|s| ScriptRow {
            name: s.name.clone(),
            status: if s.enabled { "On" } else { "Off" }.to_string(),
        })
        .collect()
}

fn script_table(rows: Vec<ScriptRow>) -> ScriptTable {
    Widget {
        state: TableStateBuilder::default().values(rows).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Scripts (t: toggle, d: delete, c: compact)".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn name_input() -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some("New Script".to_string()))
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("New Script", HorizontalAlignment::Left).into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn code_editor() -> Widget<CodeInputFieldState, CodeInputField> {
    Widget {
        state: CodeInputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some("-- select or create a script".to_string()))
            .build()
            .unwrap(),
        widget: CodeInputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Code".into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> ScriptDialog {
        ScriptDialog::new(&[ScriptDef {
            name: "boot".into(),
            code: String::new(),
            enabled: true,
        }])
    }

    #[test]
    fn c_toggles_compact_in_list_not_in_name_input() {
        let mut d = dialog();
        assert!(!d.compact);
        // `c` on the focused script list toggles compact.
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(d.compact);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(!d.compact);
        // `c` in the name input is text, not a compact toggle.
        d.focus = Focus::NewInput;
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(!d.compact);
        assert_eq!(d.name_input.state.input(), "c");
    }
}
