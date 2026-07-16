//! Shared (version-agnostic) configuration-key model and its edit dialog, used by both the OCPP
//! 1.6 config store (GetConfiguration) and the 2.0.1 variable store (GetVariables). A "key" is a
//! name/value pair with a readonly flag.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder},
    style::{InputFieldStyle, SelectionStyle},
    traits::{HandleEvents, SetFocus},
    widgets::{GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmEvent};

/// One configuration key / variable: a name, a value, and whether it is read-only.
#[derive(Clone, Debug)]
pub struct ConfigKey {
    pub key: String,
    pub value: String,
    pub readonly: bool,
}

const READONLY_CHOICES: [&str; 2] = ["false", "true"];

/// Editor for the config key at `index` in a view's config/variable store.
#[focusable]
#[derive(Focus)]
pub struct ConfigEditDialog {
    index: usize,
    #[focus]
    key: Widget<InputFieldState, InputField<String>>,
    #[focus]
    value: Widget<InputFieldState, InputField<String>>,
    #[focus]
    readonly: Widget<SelectionState<String>, Selection<String>>,
    /// Close-confirm popup, opened by Esc.
    close_confirm: Option<CloseConfirmDialog>,
    /// Set on confirmed close; the host checks this via `take_close_request` and closes the dialog.
    close_requested: bool,
}

impl ConfigEditDialog {
    pub fn new(index: usize, current: &ConfigKey) -> Self {
        Self {
            index,
            key: input("Key", &current.key),
            value: input("Value", &current.value),
            readonly: readonly_select(current.readonly),
            focus: ConfigEditDialogFocus::Key,
            view_focused: true,
            close_confirm: None,
            close_requested: false,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Route a key: the close-confirm popup captures all keys while open; Esc opens it; everything
    /// else falls through to the derived per-field routing.
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(confirm) = self.close_confirm.as_mut() {
            return match confirm.handle_key(modifiers, code) {
                CloseConfirmEvent::Close => {
                    self.close_confirm = None;
                    self.close_requested = true;
                    EventResult::Consumed
                }
                CloseConfirmEvent::Dismiss => {
                    self.close_confirm = None;
                    EventResult::Consumed
                }
                CloseConfirmEvent::Consumed => EventResult::Consumed,
            };
        }

        if modifiers == KeyModifiers::NONE && code == KeyCode::Esc {
            self.close_confirm = Some(CloseConfirmDialog::new());
            return EventResult::Consumed;
        }

        <Self as HandleEvents>::handle_events(self, modifiers, code)
    }

    /// Whether close was confirmed since the last call; clears the flag.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// The edited key. Returns `None` when the key field is empty.
    pub fn resolve(&self) -> Option<ConfigKey> {
        let key = self.key.state.input().trim().to_string();
        if key.is_empty() {
            return None;
        }
        Some(ConfigKey {
            key,
            value: self.value.state.input().trim().to_string(),
            readonly: self.readonly.state.get_value() == "true",
        })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let box_width = 50;
        let box_height = 12;
        let [_, hc, _] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(box_width),
            Constraint::Min(1),
        ])
        .areas(area);
        let [_, vc, _] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(box_height),
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
            .title("Edit Config Key");
        let block_inner = block.inner(vc);
        let inner = block_inner.inner(Margin::new(1, 0));
        UiWidget::render(&Clear, vc, buf);
        block.render(vc, buf);

        let [key_area, value_area, ro_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .areas(inner);

        self.key
            .state
            .set_focused(self.focus == ConfigEditDialogFocus::Key);
        self.value
            .state
            .set_focused(self.focus == ConfigEditDialogFocus::Value);
        self.readonly
            .state
            .set_focused(self.focus == ConfigEditDialogFocus::Readonly);

        StatefulWidget::render(&self.key.widget, key_area, buf, &mut self.key.state);
        StatefulWidget::render(&self.value.widget, value_area, buf, &mut self.value.state);
        StatefulWidget::render(
            &self.readonly.widget,
            ro_area,
            buf,
            &mut self.readonly.state,
        );

        if let Some(confirm) = self.close_confirm.as_mut() {
            confirm.render(vc, buf);
        }
    }
}

fn input(title: &str, current: &str) -> Widget<InputFieldState, InputField<String>> {
    let mut state = InputFieldStateBuilder::default()
        .focused(false)
        .disabled(false)
        .build()
        .expect("all required builder fields are set");
    state.set_input(current.to_string());
    state.set_cursor(current.chars().count());
    Widget {
        state,
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some((title, HorizontalAlignment::Left).into()))
            .style(InputFieldStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

fn readonly_select(current: bool) -> Widget<SelectionState<String>, Selection<String>> {
    let values: Vec<String> = READONLY_CHOICES.iter().map(|s| s.to_string()).collect();
    let mut state = SelectionStateBuilder::default()
        .focused(false)
        .values(values)
        .build()
        .expect("all required builder fields are set");
    state.set_selection(if current { 1 } else { 0 });
    Widget {
        state,
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Readonly", HorizontalAlignment::Left).into()))
            .style(SelectionStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ui::traits::SetFocus;

    fn dialog() -> ConfigEditDialog {
        ConfigEditDialog::new(
            0,
            &ConfigKey {
                key: "k".into(),
                value: "v".into(),
                readonly: false,
            },
        )
    }

    #[test]
    /// UI-R-023 — Esc-then-Enter sets the close request, which clears after being taken.
    fn ut_take_close_request_set_via_esc_enter_and_cleared_after_take() {
        let mut dialog = dialog();
        assert!(!dialog.take_close_request());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(dialog.take_close_request());
        assert!(!dialog.take_close_request(), "flag must clear after take");
    }

    #[test]
    /// UI-R-023 — Esc in the close-confirm keeps the config dialog open.
    fn ut_esc_in_confirm_keeps_open() {
        let mut dialog = dialog();
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_none());
        assert!(!dialog.take_close_request());
    }

    #[test]
    /// UI-R-014 — `:` types into a config text field rather than entering command mode.
    fn ut_colon_in_text_input_types() {
        let mut dialog = dialog();
        // Default focus is Key, a free-text field; `:` must be typed as ordinary text.
        dialog.set_focused(true);
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert_eq!(dialog.key.state.input(), "k:");
        assert!(dialog.close_confirm.is_none());
    }
}
