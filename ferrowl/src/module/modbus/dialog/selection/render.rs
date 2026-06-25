//! Layout and widget rendering for the selection-based register edit dialog.

use super::{EditSelectionDialog, ValueType, is_integer_format};
use ferrowl_ui::{
    COLOR_SCHEME,
    style::TextStyle,
    traits::ToLabel,
    widgets::{GetValue, TextBuilder},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};

impl<V: ToLabel + Clone> EditSelectionDialog<V> {
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.validate() {
            Ok(_) => match &self.name_error {
                Some(e) => self.error.state = e.clone(),
                None => self.error.state.clear(),
            },
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(70), Constraint::Min(1)])
                .areas(area);

        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(27 + 2 + 2 + 3 + 3 + 3 + 4),
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
            .title(if self.deletable { "Edit" } else { "Add" });
        let dialog_box = vertical_layout[1]; // preserved for sub-dialog rendering
        let area = block.inner(dialog_box).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, dialog_box, buf);
        block.render(dialog_box, buf);

        let mut vertical_index = 0;
        let vertical_layout: [Rect; 13] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        StatefulWidget::render(
            &self.label.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.label.state,
        );
        vertical_index += 1;

        StatefulWidget::render(
            &self.description.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.description.state,
        );
        vertical_index += 1;

        let horizontal_layout: [Rect; 2] =
            Layout::horizontal([Constraint::Min(1), Constraint::Min(1)])
                .areas(vertical_layout[vertical_index]);
        vertical_index += 1;

        StatefulWidget::render(
            &self.slave_id.widget,
            horizontal_layout[0],
            buf,
            &mut self.slave_id.state,
        );

        StatefulWidget::render(
            &self.address.widget,
            horizontal_layout[1],
            buf,
            &mut self.address.state,
        );

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Min(1), Constraint::Min(1)])
                .areas(vertical_layout[vertical_index]);
        vertical_index += 1;

        StatefulWidget::render(
            &self.kind.widget,
            horizontal_layout[0],
            buf,
            &mut self.kind.state,
        );

        StatefulWidget::render(
            &self.access.widget,
            horizontal_layout[1],
            buf,
            &mut self.access.state,
        );

        StatefulWidget::render(
            &self.value_type.widget,
            horizontal_layout[2],
            buf,
            &mut self.value_type.state,
        );

        match self.value_type.state.values()[self.value_type.state.selection()] {
            ValueType::Number => {
                // Integer formats get a 4th column for the bitmask; floats keep 3.
                let integer = is_integer_format(&self.number_format.get_value().0);
                let columns = if integer { 4 } else { 3 };
                let cells = Layout::horizontal(vec![Constraint::Min(1); columns])
                    .split(vertical_layout[vertical_index]);

                StatefulWidget::render(
                    &self.number_format.widget,
                    cells[0],
                    buf,
                    &mut self.number_format.state,
                );

                StatefulWidget::render(
                    &self.number_endian.widget,
                    cells[1],
                    buf,
                    &mut self.number_endian.state,
                );

                StatefulWidget::render(
                    &self.number_resolution.widget,
                    cells[2],
                    buf,
                    &mut self.number_resolution.state,
                );

                if integer {
                    StatefulWidget::render(
                        &self.number_bitmask.widget,
                        cells[3],
                        buf,
                        &mut self.number_bitmask.state,
                    );
                }
            }
            ValueType::Text => {
                let horizontal_layout: [Rect; 2] =
                    Layout::horizontal([Constraint::Min(1), Constraint::Min(1)])
                        .areas(vertical_layout[vertical_index]);

                StatefulWidget::render(
                    &self.text_alignment.widget,
                    horizontal_layout[0],
                    buf,
                    &mut self.text_alignment.state,
                );

                StatefulWidget::render(
                    &self.text_width.widget,
                    horizontal_layout[1],
                    buf,
                    &mut self.text_width.state,
                );
            }
        }
        vertical_index += 1;

        // Value selection + ADD + DEL buttons side by side
        let horizontal_layout: [Rect; 4] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .areas(vertical_layout[vertical_index]);

        if self.value.state.values().is_empty() {
            let text = TextBuilder::default()
                .margin(Margin {
                    horizontal: 1,
                    vertical: 0,
                })
                .horizontal_alignment(HorizontalAlignment::Center)
                .style(TextStyle {
                    general: ratatui::style::Style::default()
                        .fg(COLOR_SCHEME.hi)
                        .bg(COLOR_SCHEME.bg),
                })
                .multiline(true)
                .build()
                .unwrap();
            let mut message: String = "No predefined values — reopen to use free-text input".into();
            StatefulWidget::render(&text, horizontal_layout[0], buf, &mut message);
        } else {
            StatefulWidget::render(
                &self.value.widget,
                horizontal_layout[0],
                buf,
                &mut self.value.state,
            );
            StatefulWidget::render(
                &self.delete_button.widget,
                horizontal_layout[2],
                buf,
                &mut self.delete_button.state,
            );
        }

        StatefulWidget::render(
            &self.add_button.widget,
            horizontal_layout[1],
            buf,
            &mut self.add_button.state,
        );

        vertical_index += 1;

        if !self.default_value.state.values().is_empty() {
            StatefulWidget::render(
                &self.default_value.widget,
                vertical_layout[vertical_index],
                buf,
                &mut self.default_value.state,
            );
        }
        vertical_index += 1;

        StatefulWidget::render(
            &self.update_script.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.update_script.state,
        );
        vertical_index += 1;

        if self.deletable {
            let buttons: [Rect; 2] =
                Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .areas(vertical_layout[vertical_index]);
            StatefulWidget::render(
                &self.confirm_button.widget,
                buttons[0],
                buf,
                &mut self.confirm_button.state,
            );
            StatefulWidget::render(
                &self.delete_register_button.widget,
                buttons[1],
                buf,
                &mut self.delete_register_button.state,
            );
        } else {
            StatefulWidget::render(
                &self.confirm_button.widget,
                vertical_layout[vertical_index],
                buf,
                &mut self.confirm_button.state,
            );
        }
        vertical_index += 1;

        if !self.error.state.is_empty() {
            StatefulWidget::render(
                &self.error.widget,
                vertical_layout[vertical_index],
                buf,
                &mut self.error.state,
            );
        } else {
            StatefulWidget::render(
                &self.success.widget,
                vertical_layout[vertical_index],
                buf,
                &mut self.success.state,
            );
        }
        vertical_index += 2;

        StatefulWidget::render(
            &self.keybinds[0].widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.keybinds[0].state,
        );
        vertical_index += 1;
        StatefulWidget::render(
            &self.keybinds[1].widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.keybinds[1].state,
        );

        // Render add sub-dialog on top if open — centred within the main dialog box.
        if let Some(d) = self.add_dialog.as_mut() {
            d.render(dialog_box, buf);
        }

        // Render the delete-confirmation box on top if open.
        if let Some(d) = self.confirm_delete.as_mut() {
            d.render(dialog_box, buf);
        }
    }
}
