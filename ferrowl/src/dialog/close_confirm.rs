//! Reusable confirm-close popup: small dialog asking whether to close the underlying dialog.
//! Confirm-close popup opened by Esc on a top-level dialog.
#![allow(dead_code)]

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{ButtonState, ButtonStateBuilder},
    style::{ButtonStyle, TextStyle},
    widgets::{Button, ButtonBuilder, Text, TextBuilder, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};

/// Outcome of feeding a key into a [`CloseConfirmDialog`].
#[derive(Debug, PartialEq, Eq)]
pub enum CloseConfirmEvent {
    /// Key eaten, confirm stays open.
    Consumed,
    /// Enter or Space: host should close the underlying dialog.
    Close,
    /// Esc: host should drop the confirm; the underlying dialog stays open.
    Dismiss,
}

// ---------------------------------------------------------------------------
// CloseConfirmDialog — small popup guarding accidental dialog close
// ---------------------------------------------------------------------------

#[derive(Builder, Clone, Debug)]
pub struct CloseConfirmDialog {
    // Single CLOSE button, always focused.
    close_button: Widget<ButtonState, Button>,
    // Keybind hint.
    keybinds: Widget<String, Text>,
}

impl CloseConfirmDialog {
    pub fn new() -> Self {
        let text_style = TextStyle::default();
        let button_style = ButtonStyle::default();

        CloseConfirmDialogBuilder::default()
            .close_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(true)
                    .label("CLOSE".to_string())
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
                state: "<Enter>: close | <Esc>: cancel".to_string(),
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
            .build()
            .unwrap()
    }

    /// Feed one key while the confirm is open.
    pub fn handle_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> CloseConfirmEvent {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                CloseConfirmEvent::Close
            }
            (KeyModifiers::NONE, KeyCode::Esc) => CloseConfirmEvent::Dismiss,
            _ => CloseConfirmEvent::Consumed,
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let horizontal_layout: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(46),
            Constraint::Min(1),
        ])
        .areas(area);

        // 2 border + 2 margin-vertical + 3 button + 1 keybinds = 8
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(8),
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
            .title(" Confirm Close ");

        let inner = block.inner(vertical_layout[1]).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vertical_layout[1], buf);
        block.render(vertical_layout[1], buf);

        let inner_layout: [Rect; 2] =
            Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).areas(inner);

        StatefulWidget::render(
            &self.close_button.widget,
            inner_layout[0],
            buf,
            &mut self.close_button.state,
        );

        StatefulWidget::render(
            &self.keybinds.widget,
            inner_layout[1],
            buf,
            &mut self.keybinds.state,
        );
    }
}

impl Default for CloseConfirmDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_enter_closes() {
        let mut d = CloseConfirmDialog::new();
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Enter),
            CloseConfirmEvent::Close
        );
    }

    #[test]
    fn ut_space_closes() {
        let mut d = CloseConfirmDialog::new();
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Char(' ')),
            CloseConfirmEvent::Close
        );
    }

    #[test]
    fn ut_esc_dismisses() {
        let mut d = CloseConfirmDialog::new();
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Esc),
            CloseConfirmEvent::Dismiss
        );
    }

    #[test]
    fn ut_modified_keys_consumed() {
        let mut d = CloseConfirmDialog::new();
        for code in [KeyCode::Enter, KeyCode::Char(' '), KeyCode::Esc] {
            for modifiers in [KeyModifiers::SHIFT, KeyModifiers::CONTROL] {
                assert_eq!(
                    d.handle_key(modifiers, code),
                    CloseConfirmEvent::Consumed,
                    "{modifiers:?}+{code:?} must be consumed"
                );
            }
        }
    }

    #[test]
    fn ut_other_keys_consumed() {
        let mut d = CloseConfirmDialog::new();
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Char('q')),
            CloseConfirmEvent::Consumed
        );
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Char('a')),
            CloseConfirmEvent::Consumed
        );
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Tab),
            CloseConfirmEvent::Consumed
        );
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Left),
            CloseConfirmEvent::Consumed
        );
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Right),
            CloseConfirmEvent::Consumed
        );
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Up),
            CloseConfirmEvent::Consumed
        );
        assert_eq!(
            d.handle_key(KeyModifiers::NONE, KeyCode::Down),
            CloseConfirmEvent::Consumed
        );
    }
}
