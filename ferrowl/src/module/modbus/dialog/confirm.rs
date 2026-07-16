//! Yes/no confirmation dialog guarding register deletion.

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{ButtonState, ButtonStateBuilder},
    style::{ButtonStyle, TextStyle},
    traits::HandleEvents,
    widgets::{Button, ButtonBuilder, Text, TextBuilder, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

/// Outcome of [`route_delete_confirm`]: whether a delete-confirmation popup was open, and if so,
/// whether the caller should now perform the delete.
#[derive(Debug, PartialEq, Eq)]
pub enum DeleteConfirmOutcome {
    /// No confirm popup was open; the key wasn't touched, the caller should route it itself.
    NotActive,
    /// The user confirmed: the caller should now delete the selected item.
    Confirmed,
    /// The popup captured the key (or was just dismissed/navigated) and the caller should stop
    /// routing this key further.
    Consumed,
}

/// Feed one key through `confirm`, if a delete-confirmation popup is currently open. Esc cancels,
/// Tab/BackTab switch focus, Enter/Space selects the focused button (clearing the popup either
/// way); anything else is offered to the popup's own event handling. Returns
/// [`DeleteConfirmOutcome::Confirmed`] when the DELETE button was selected — the caller is
/// responsible for performing the delete, since this popup carries no reference to the item it
/// guards.
pub fn route_delete_confirm(
    confirm: &mut Option<ConfirmDeleteDialog>,
    modifiers: KeyModifiers,
    code: KeyCode,
) -> DeleteConfirmOutcome {
    let Some(c) = confirm.as_mut() else {
        return DeleteConfirmOutcome::NotActive;
    };
    match (modifiers, code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            *confirm = None;
            DeleteConfirmOutcome::Consumed
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            c.focus_next();
            DeleteConfirmOutcome::Consumed
        }
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
            c.focus_previous();
            DeleteConfirmOutcome::Consumed
        }
        (KeyModifiers::NONE, KeyCode::Enter | KeyCode::Char(' ')) => {
            let confirmed = c.is_confirm_focused();
            *confirm = None;
            if confirmed {
                DeleteConfirmOutcome::Confirmed
            } else {
                DeleteConfirmOutcome::Consumed
            }
        }
        _ => {
            let _ = c.handle_events(modifiers, code);
            DeleteConfirmOutcome::Consumed
        }
    }
}

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
                    .expect("all required builder fields are set"),
            })
            .cancel_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(true)
                    .label("CANCEL".to_string())
                    .disabled(false)
                    .build()
                    .expect("all required builder fields are set"),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(button_style.clone())
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .delete_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("DELETE".to_string())
                    .disabled(false)
                    .build()
                    .expect("all required builder fields are set"),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(button_style)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .expect("all required builder fields are set"),
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
                    .expect("all required builder fields are set"),
            })
            .focus(ConfirmDeleteDialogFocus::CancelButton)
            .build()
            .expect("all required builder fields are set")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_route_not_active_when_none() {
        let mut confirm: Option<ConfirmDeleteDialog> = None;
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Enter),
            DeleteConfirmOutcome::NotActive
        );
    }

    #[test]
    fn ut_route_esc_cancels_and_clears() {
        let mut confirm = Some(ConfirmDeleteDialog::new("reg"));
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Esc),
            DeleteConfirmOutcome::Consumed
        );
        assert!(confirm.is_none());
    }

    #[test]
    fn ut_route_enter_on_cancel_focused_is_consumed_not_confirmed() {
        let mut confirm = Some(ConfirmDeleteDialog::new("reg"));
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Enter),
            DeleteConfirmOutcome::Consumed
        );
        assert!(confirm.is_none());
    }

    #[test]
    fn ut_route_tab_then_enter_confirms() {
        let mut confirm = Some(ConfirmDeleteDialog::new("reg"));
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Tab),
            DeleteConfirmOutcome::Consumed
        );
        assert!(confirm.is_some());
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Enter),
            DeleteConfirmOutcome::Confirmed
        );
        assert!(confirm.is_none());
    }

    #[test]
    fn ut_route_space_confirms_when_delete_focused() {
        let mut confirm = Some(ConfirmDeleteDialog::new("reg"));
        confirm.as_mut().unwrap().focus_next();
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Char(' ')),
            DeleteConfirmOutcome::Confirmed
        );
        assert!(confirm.is_none());
    }

    #[test]
    fn ut_route_other_key_stays_open() {
        let mut confirm = Some(ConfirmDeleteDialog::new("reg"));
        assert_eq!(
            route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Char('x')),
            DeleteConfirmOutcome::Consumed
        );
        assert!(confirm.is_some());
    }

    #[test]
    fn ut_confirm_focus_starts_on_cancel_and_tab_moves_to_delete() {
        let mut confirm = Some(ConfirmDeleteDialog::new("reg"));
        assert!(!confirm.as_ref().unwrap().is_confirm_focused());
        route_delete_confirm(&mut confirm, KeyModifiers::NONE, KeyCode::Tab);
        assert!(confirm.as_ref().unwrap().is_confirm_focused());
    }

    #[test]
    fn ut_render_names_the_register_in_a_titled_modal() {
        let mut dialog = ConfirmDeleteDialog::new("my_reg");
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Confirm Delete"));
        assert!(text.contains("my_reg"));
    }
}
