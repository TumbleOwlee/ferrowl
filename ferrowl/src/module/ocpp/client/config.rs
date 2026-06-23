//! Shared (version-agnostic) configuration-key model and its edit dialog, used by both the OCPP
//! 1.6 config store (GetConfiguration) and the 2.0.1 variable store (GetVariables). A "key" is a
//! name/value pair with a readonly flag.

use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder},
    style::{InputFieldStyle, SelectionStyle},
    widgets::{GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

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
        }
    }

    pub fn index(&self) -> usize {
        self.index
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
