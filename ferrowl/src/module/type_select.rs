//! Module-type selector — the first step of the creation flow (`:n`/`:new`).
//!
//! Lists the available module types from the [`MODULE_TYPES`](crate::module::MODULE_TYPES)
//! registry and lets the user pick one; confirming opens that type's own setup dialog.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{SelectionState, SelectionStateBuilder},
    style::{SelectionStyle, TextStyle},
    traits::{HandleEvents, ToLabel},
    widgets::{Selection, SelectionBuilder, Text, TextBuilder, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::module::MODULE_TYPES;

/// One entry in the type list: a module type's display label. The entry's position
/// matches its index in [`MODULE_TYPES`].
#[derive(Debug, Clone)]
pub struct TypeChoice {
    pub label: &'static str,
}

impl ToLabel for TypeChoice {
    fn to_label(&self) -> String {
        self.label.to_string()
    }
}

/// Centered modal listing the registered module types.
pub struct TypeSelectDialog {
    selection: Widget<SelectionState<TypeChoice>, Selection<TypeChoice>>,
    keybinds: [Widget<String, Text>; 2],
}

impl TypeSelectDialog {
    pub fn new() -> Self {
        let values: Vec<TypeChoice> = MODULE_TYPES
            .iter()
            .map(|descriptor| TypeChoice {
                label: descriptor.label,
            })
            .collect();

        let selection = Widget {
            state: SelectionStateBuilder::default()
                .focused(true)
                .values(values)
                .build()
                .unwrap(),
            widget: SelectionBuilder::default()
                .border(Border::Full(Margin::new(1, 0)))
                .title(Some(("Type", HorizontalAlignment::Left).into()))
                .margin(Margin {
                    vertical: 0,
                    horizontal: 1,
                })
                .style(SelectionStyle::default())
                .build()
                .unwrap(),
        };

        let keybinds = [
            Widget {
                state: "<\u{2191}/\u{2193}>: select | <Enter>: confirm".to_string(),
                widget: TextBuilder::default()
                    .multiline(true)
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(TextStyle::default())
                    .build()
                    .unwrap(),
            },
            Widget {
                state: "<Esc>: cancel".to_string(),
                widget: TextBuilder::default()
                    .multiline(true)
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(TextStyle::default())
                    .build()
                    .unwrap(),
            },
        ];

        Self {
            selection,
            keybinds,
        }
    }

    /// Index into [`MODULE_TYPES`] of the highlighted entry.
    pub fn selected_index(&self) -> usize {
        self.selection.state.selection()
    }

    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let _ = self.selection.state.handle_events(modifiers, code);
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // outer border (2) + selection box [border (2) + 1] + 2 keybind lines.
        let box_height = 7;
        let box_width = 40;

        let horizontal: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(box_width),
            Constraint::Min(1),
        ])
        .areas(area);
        let vertical: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(box_height),
            Constraint::Min(1),
        ])
        .areas(horizontal[1]);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("New Module");

        let inner = block.inner(vertical[1]);
        UiWidget::render(&Clear, vertical[1], buf);
        block.render(vertical[1], buf);

        let rows: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.selection.widget,
            rows[0],
            buf,
            &mut self.selection.state,
        );
        StatefulWidget::render(
            &self.keybinds[0].widget,
            rows[1],
            buf,
            &mut self.keybinds[0].state,
        );
        StatefulWidget::render(
            &self.keybinds[1].widget,
            rows[2],
            buf,
            &mut self.keybinds[1].state,
        );
    }
}
