//! Layout and widget rendering for the free-text register edit dialog.

use super::{EditInputDialog, ValueType, is_integer_format};
use ferrowl_ui::COLOR_SCHEME;
use ferrowl_ui::widgets::GetValue;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};

impl EditInputDialog {
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Show error: field-level validation takes precedence; otherwise surface a pending
        // name-conflict error set by the app at confirm time.
        match self.validate() {
            Ok(_) => match &self.name_error {
                Some(e) => self.error.state = e.clone(),
                None => self.error.state.clear(),
            },
            Err(e) => {
                self.error.state = e;
            }
        }

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(70), Constraint::Min(1)])
                .areas(area);

        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(48),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .bg(COLOR_SCHEME.bg)
                    .fg(COLOR_SCHEME.hi),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title(if self.deletable { "Edit" } else { "Add" });
        let dialog_box = vertical_layout[1];
        let area = block.inner(dialog_box).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, dialog_box, buf);
        block.render(dialog_box, buf);

        let mut vertical_index = 0;
        let vertical_layout: [Rect; 14] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(4),
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

        if self.is_boolean_kind() {
            StatefulWidget::render(
                &self.boolean_type.widget,
                horizontal_layout[2],
                buf,
                &mut self.boolean_type.state,
            );
        } else {
            StatefulWidget::render(
                &self.value_type.widget,
                horizontal_layout[2],
                buf,
                &mut self.value_type.state,
            );
        }

        if !self.is_boolean_kind() {
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
        }
        vertical_index += 1;

        StatefulWidget::render(
            &self.value.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.value.state,
        );
        vertical_index += 1;

        StatefulWidget::render(
            &self.default_value.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.default_value.state,
        );
        vertical_index += 1;

        StatefulWidget::render(
            &self.add_button.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.add_button.state,
        );
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

        if let Some(d) = self.add_dialog.as_mut() {
            d.render(dialog_box, buf);
        }

        if let Some(d) = self.confirm_delete.as_mut() {
            d.render(dialog_box, buf);
        }
    }
}

#[cfg(test)]
mod render_tests {
    //! Render-to-buffer characterization: the dialog draws its box title, fields, and
    //! validation-error text without panicking, in both Add and Edit configurations.
    use super::EditInputDialog;
    use ferrowl_codec::format::{BitField, Endian, Format, Resolution};
    use ferrowl_codec::{Access, Address, Kind, RegisterBuilder};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn render(dialog: &mut EditInputDialog) -> String {
        let area = Rect::new(0, 0, 80, 54);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        buffer_text(&buf)
    }

    #[test]
    fn ut_render_add_dialog_shows_title_fields_and_validation_error() {
        let mut dialog = EditInputDialog::new();
        let text = render(&mut dialog);
        assert!(text.contains("Add"), "missing box title:\n{text}");
        for field in ["Label", "Slave ID", "Address", "Kind", "Access", "CONFIRM"] {
            assert!(text.contains(field), "missing field '{field}':\n{text}");
        }
        // A fresh dialog has an empty slave ID -> the validation error is surfaced in the
        // Error pane instead of panicking. (`validate()` checks label via `String::validate`,
        // which never errors — slave ID is the first field that actually rejects empty input.)
        assert!(
            text.contains("Slave ID:"),
            "missing validation error:\n{text}"
        );
    }

    #[test]
    fn ut_render_edit_dialog_shows_edit_title_and_delete_button() {
        let register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(Format::U16((
                Endian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();
        let mut dialog = EditInputDialog::from_register("temp", "d", &register, "42", None, None);
        let text = render(&mut dialog);
        assert!(text.contains("Edit"), "missing box title:\n{text}");
        assert!(text.contains("DELETE"), "missing delete button:\n{text}");
        // Pre-filled fields are drawn.
        assert!(text.contains("temp"), "missing label value:\n{text}");
        assert!(text.contains("42"), "missing value:\n{text}");
    }
}
