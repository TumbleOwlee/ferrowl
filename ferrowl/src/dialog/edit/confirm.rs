//! Yes/no confirmation dialog guarding register deletion.

use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{ButtonState, ButtonStateBuilder},
    style::{ButtonStyle, TextStyle},
    types::Border,
    widgets::{Button, ButtonBuilder, Text, TextBuilder, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

// ---------------------------------------------------------------------------
// ConfirmDeleteDialog — small inline yes/no box guarding register deletion
// ---------------------------------------------------------------------------

#[focusable]
#[derive(Builder, Clone, Debug, Focus)]
pub struct ConfirmDeleteDialog {
    // Warning message naming the register about to be deleted.
    pub message: Widget<String, Text>,
    // Cancel button (focused by default — the safe choice).
    #[focus]
    pub cancel_button: Widget<ButtonState, Button>,
    // Confirm-deletion button.
    #[focus]
    pub delete_button: Widget<ButtonState, Button>,
    // Keybind hint.
    pub keybinds: Widget<String, Text>,
}

impl ConfirmDeleteDialog {
    pub fn new(register_name: &str) -> Self {
        let text_style = TextStyle::default();
        let warn_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let button_style = ButtonStyle::default();

        let message = if register_name.is_empty() {
            "Delete this register completely? This cannot be undone.".to_string()
        } else {
            format!("Delete register '{register_name}' completely? This cannot be undone.")
        };

        ConfirmDeleteDialogBuilder::default()
            .message(Widget {
                state: message,
                widget: TextBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Warning".into()))
                    .multiline(true)
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(warn_style)
                    .build()
                    .unwrap(),
            })
            .cancel_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(true)
                    .label("CANCEL".to_string())
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(button_style.clone())
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .unwrap(),
            })
            .delete_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("DELETE".to_string())
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(button_style)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .unwrap(),
            })
            .keybinds(Widget {
                state: "<Tab>: switch | <Space>/<Enter>: select | <Esc>: cancel".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(text_style)
                    .build()
                    .unwrap(),
            })
            .focus(ConfirmDeleteDialogFocus::CancelButton)
            .build()
            .unwrap()
    }

    /// Whether the DELETE button (rather than CANCEL) currently holds focus.
    pub fn is_confirm_focused(&self) -> bool {
        matches!(self.focus, ConfirmDeleteDialogFocus::DeleteButton)
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let horizontal_layout: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(60),
            Constraint::Min(1),
        ])
        .areas(area);

        // 2 border + 2 margin-vertical + 4 message + 3 buttons + 1 keybinds = 12
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(12),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("Confirm Delete");

        let inner = block.inner(vertical_layout[1]).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vertical_layout[1], buf);
        block.render(vertical_layout[1], buf);

        let inner_layout: [Rect; 3] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.message.widget,
            inner_layout[0],
            buf,
            &mut self.message.state,
        );

        let button_row: [Rect; 2] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(inner_layout[1]);
        StatefulWidget::render(
            &self.cancel_button.widget,
            button_row[0],
            buf,
            &mut self.cancel_button.state,
        );
        StatefulWidget::render(
            &self.delete_button.widget,
            button_row[1],
            buf,
            &mut self.delete_button.state,
        );

        StatefulWidget::render(
            &self.keybinds.widget,
            inner_layout[2],
            buf,
            &mut self.keybinds.state,
        );
    }
}
