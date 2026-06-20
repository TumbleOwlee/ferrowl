//! Modal dialog to edit one OCPP 1.6 configuration key (key / value / readonly), opened by
//! selecting a row in the config table. Three fields cycled with Tab; Enter confirms, Esc cancels.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder},
    style::{InputFieldStyle, SelectionStyle},
    traits::HandleEvents,
    widgets::{GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::module::ocpp::client::v1_6::state::ConfigKey;

/// Which dialog field has focus.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Key,
    Value,
    Readonly,
}

const READONLY_CHOICES: [&str; 2] = ["false", "true"];

/// Editor for the config key at `index` in `CsState.config`.
pub struct ConfigEditDialog {
    index: usize,
    key: Widget<InputFieldState, InputField<String>>,
    value: Widget<InputFieldState, InputField<String>>,
    readonly: Widget<SelectionState<String>, Selection<String>>,
    focus: Field,
}

impl ConfigEditDialog {
    pub fn new(index: usize, current: &ConfigKey) -> Self {
        Self {
            index,
            key: input("Key", &current.key),
            value: input("Value", &current.value),
            readonly: readonly_select(current.readonly),
            focus: Field::Key,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Field::Key => Field::Value,
            Field::Value => Field::Readonly,
            Field::Readonly => Field::Key,
        };
    }

    pub fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Field::Key => Field::Readonly,
            Field::Value => Field::Key,
            Field::Readonly => Field::Value,
        };
    }

    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self.focus {
            Field::Key => {
                let _ = self.key.state.handle_events(modifiers, code);
            }
            Field::Value => {
                let _ = self.value.state.handle_events(modifiers, code);
            }
            Field::Readonly => {
                let _ = self.readonly.state.handle_events(modifiers, code);
            }
        }
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
        let inner = block.inner(vc).inner(Margin::new(1, 0));
        UiWidget::render(&Clear, vc, buf);
        block.render(vc, buf);

        let [key_area, value_area, ro_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .areas(inner);

        self.key.state.set_focused(self.focus == Field::Key);
        self.value.state.set_focused(self.focus == Field::Value);
        self.readonly
            .state
            .set_focused(self.focus == Field::Readonly);

        StatefulWidget::render(&self.key.widget, key_area, buf, &mut self.key.state);
        StatefulWidget::render(&self.value.widget, value_area, buf, &mut self.value.state);
        StatefulWidget::render(
            &self.readonly.widget,
            ro_area,
            buf,
            &mut self.readonly.state,
        );
    }
}

fn input(title: &str, current: &str) -> Widget<InputFieldState, InputField<String>> {
    let mut state = InputFieldStateBuilder::default()
        .focused(false)
        .disabled(false)
        .build()
        .unwrap();
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
            .unwrap(),
    }
}

fn readonly_select(current: bool) -> Widget<SelectionState<String>, Selection<String>> {
    let values: Vec<String> = READONLY_CHOICES.iter().map(|s| s.to_string()).collect();
    let mut state = SelectionStateBuilder::default()
        .focused(false)
        .values(values)
        .build()
        .unwrap();
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
            .unwrap(),
    }
}
