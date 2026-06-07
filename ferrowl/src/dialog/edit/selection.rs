use crate::config::device::NamedValue;
use crate::dialog::EditedRegister;
use crate::dialog::edit::{
    Alignment, Endian, Format, ValueType, alignment_index, endian_index, format_index,
    numeric_parts, set_input, with_endian_resolution,
};
use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_reg::format::{
    Alignment as TextAlignment, Endian as RegisterEndian, Format as RegisterFormat, Resolution,
    Width,
};
use ferrowl_reg::{Access, Address, Kind, Register, RegisterBuilder};
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{
        ButtonState, ButtonStateBuilder, InputFieldState, InputFieldStateBuilder, SelectionState,
        SelectionStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    traits::HandleEvents,
    traits::ToLabel,
    types::Border,
    widgets::{
        Button, ButtonBuilder, GetValue, InputField, InputFieldBuilder, Selection,
        SelectionBuilder, Text, TextBuilder, Validate, Widget,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    style::palette::tailwind,
    widgets::{Block, Paragraph, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

/// Parse a raw memory string like `[00a0 0001]` into an i64 (big-endian word combination).
fn parse_raw_value(raw: &str) -> Option<i64> {
    let inner = raw.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut result: u64 = 0;
    for word in inner.split_whitespace() {
        result = (result << 16) | u64::from_str_radix(word, 16).ok()?;
    }
    Some(result as i64)
}

// ---------------------------------------------------------------------------
// AddNamedValueDialog — small inline sub-dialog for creating a new NamedValue
// ---------------------------------------------------------------------------

#[focusable]
#[derive(Builder, Clone, Debug, Focus)]
pub struct AddNamedValueDialog {
    #[focus]
    pub label: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub value: Widget<InputFieldState, InputField<i64>>,
    // Error display field
    pub error: Widget<String, Text>,
    pub keybinds: [Widget<String, Text>; 2],
}

impl AddNamedValueDialog {
    pub fn new() -> Self {
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let text_style = TextStyle::default();

        AddNamedValueDialogBuilder::default()
            .label(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(true)
                    .disabled(false)
                    .placeholder(Some("Name...".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Label".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .value(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("0".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Value".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .error(Widget {
                state: "".to_string(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(error_style.clone())
                    .build()
                    .unwrap(),
            })
            .keybinds([
                Widget {
                    state: "<Tab>: next".to_string(),
                    widget: TextBuilder::default()
                        .margin(Margin {
                            vertical: 0,
                            horizontal: 1,
                        })
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .style(text_style.clone())
                        .build()
                        .unwrap(),
                },
                Widget {
                    state: "<Esc>: cancel | <Enter>: confirm".to_string(),
                    widget: TextBuilder::default()
                        .margin(Margin {
                            vertical: 0,
                            horizontal: 1,
                        })
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .style(text_style.clone())
                        .build()
                        .unwrap(),
                },
            ])
            .focus(AddNamedValueDialogFocus::Label)
            .build()
            .unwrap()
    }

    fn validate(&self) -> Result<(), String> {
        if let Err(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        }
        if self.label.state.input().trim().is_empty() {
            return Err("Label must not be empty.".to_string());
        }
        if let Err(e) = i64::validate(self.value.state.input()) {
            return Err(format!("Value: {e}"));
        }
        Ok(())
    }

    pub fn apply(&self) -> Result<NamedValue, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let value: i64 = self
            .value
            .state
            .input()
            .trim()
            .parse()
            .map_err(|_| "Value must be an integer.".to_string())?;
        Ok(NamedValue { name, value })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Show validation error inline.
        match self.validate() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(60),
            Constraint::Min(1),
        ])
        .areas(area);

        // 2 border + 2 margin-vertical + 3 label + 3 value + 1 error + 1 keybinds = 12
        let error_height = if self.error.state.is_empty() { 0 } else { 3 };
        let total_height = 2 + 2 + 3 + 3 + error_height + 1 + 1 + 1;
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(total_height),
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
            .title("Add Value");

        let inner = block.inner(vertical_layout[1]).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vertical_layout[1], buf);
        block.render(vertical_layout[1], buf);

        let inner_layout: [Rect; 6] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(error_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.label.widget,
            inner_layout[0],
            buf,
            &mut self.label.state,
        );
        StatefulWidget::render(
            &self.value.widget,
            inner_layout[1],
            buf,
            &mut self.value.state,
        );
        if !self.error.state.is_empty() {
            StatefulWidget::render(
                &self.error.widget,
                inner_layout[2],
                buf,
                &mut self.error.state,
            );
        }
        StatefulWidget::render(
            &self.keybinds[0].widget,
            inner_layout[4],
            buf,
            &mut self.keybinds[0].state,
        );
        StatefulWidget::render(
            &self.keybinds[1].widget,
            inner_layout[5],
            buf,
            &mut self.keybinds[1].state,
        );
    }
}

// ---------------------------------------------------------------------------
// EditSelectionDialog
// ---------------------------------------------------------------------------

#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct EditSelectionDialog<V>
where
    V: ToLabel + Clone,
{
    // Label for the register
    #[focus]
    pub label: Widget<InputFieldState, InputField<String>>,
    // Description for the register
    #[focus]
    pub description: Widget<InputFieldState, InputField<String>>,
    // Address of the start register
    #[focus]
    pub address: Widget<InputFieldState, InputField<u16>>,
    // Type selection
    #[focus]
    pub value_type: Widget<SelectionState<ValueType>, Selection<ValueType>>,
    // Number format selection
    #[focus(when = {self.value_type.get_value() == ValueType::Number})]
    pub number_format: Widget<SelectionState<Format>, Selection<Format>>,
    // Number endianess selection
    #[focus(when = {self.value_type.get_value() == ValueType::Number})]
    pub number_endian: Widget<SelectionState<Endian>, Selection<Endian>>,
    // Number resolution input
    #[focus(when = {self.value_type.get_value() == ValueType::Number})]
    pub number_resolution: Widget<InputFieldState, InputField<f64>>,
    // Text alignment selection
    #[focus(when = {self.value_type.get_value() == ValueType::Text})]
    pub text_alignment: Widget<SelectionState<Alignment>, Selection<Alignment>>,
    // Text length input
    #[focus(when = {self.value_type.get_value() == ValueType::Text})]
    pub text_width: Widget<InputFieldState, InputField<usize>>,
    // Value selection
    #[focus]
    pub value: Widget<SelectionState<V>, Selection<V>>,
    // Add button
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    // Delete button
    #[focus]
    pub delete_button: Widget<ButtonState, Button>,
    // Lua simulation script (optional multiline)
    #[focus]
    pub update_script: Widget<InputFieldState, InputField<String>>,
    // Error display field
    pub error: Widget<String, Text>,
    // Success display field
    pub success: Widget<String, Text>,
    // Keybinds display field
    pub keybinds: [Widget<String, Text>; 2],
    // Register metadata preserved across edits
    #[builder(default)]
    pub base_slave_id: u8,
    #[builder(default = "Access::ReadWrite")]
    pub base_access: Access,
    #[builder(default = "Kind::HoldingRegister")]
    pub base_kind: Kind,
    // Optional add-value sub-dialog
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
}

impl<V: ToLabel + Clone> EditSelectionDialog<V> {
    fn validate(&self) -> Result<(), String> {
        if let Err(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        } else if let Err(e) = u16::validate(self.address.state.input()) {
            return Err(format!("Address: {e}"));
        }

        match self.value_type.state.values()[self.value_type.state.selection()] {
            ValueType::Number => {
                if let Err(e) = f64::validate(self.number_resolution.state.input()) {
                    return Err(format!("Resolution: {e}"));
                }
            }
            ValueType::Text => {
                if let Err(e) = usize::validate(self.text_width.state.input()) {
                    return Err(format!("Width: {e}"));
                }
            }
        }
        Ok(())
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.validate() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(70), Constraint::Min(1)])
                .areas(area);

        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(27 + 2 + 2),
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
            .title("Edit");
        let dialog_box = vertical_layout[1]; // preserved for sub-dialog rendering
        let area = block.inner(dialog_box).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, dialog_box, buf);
        block.render(dialog_box, buf);

        let mut vertical_index = 0;
        let vertical_layout: [Rect; 10] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(6),
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
            &self.address.widget,
            horizontal_layout[0],
            buf,
            &mut self.address.state,
        );

        StatefulWidget::render(
            &self.value_type.widget,
            horizontal_layout[1],
            buf,
            &mut self.value_type.state,
        );

        match self.value_type.state.values()[self.value_type.state.selection()] {
            ValueType::Number => {
                let horizontal_layout: [Rect; 3] = Layout::horizontal([
                    Constraint::Min(1),
                    Constraint::Min(1),
                    Constraint::Min(1),
                ])
                .areas(vertical_layout[vertical_index]);

                StatefulWidget::render(
                    &self.number_format.widget,
                    horizontal_layout[0],
                    buf,
                    &mut self.number_format.state,
                );

                StatefulWidget::render(
                    &self.number_endian.widget,
                    horizontal_layout[1],
                    buf,
                    &mut self.number_endian.state,
                );

                StatefulWidget::render(
                    &self.number_resolution.widget,
                    horizontal_layout[2],
                    buf,
                    &mut self.number_resolution.state,
                );
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

        StatefulWidget::render(
            &self.update_script.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.update_script.state,
        );
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
    }

    pub fn new(values: Vec<V>) -> Self {
        let selection_style = SelectionStyle::default();
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let success_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.success)
                .bg(COLOR_SCHEME.bg),
        };
        let text_style = TextStyle::default();
        let button_style = ButtonStyle::default();

        EditSelectionDialogBuilder::<V>::default()
            .label(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("Custom label...".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Label".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .description(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("Some description...".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Description".into()))
                    .multiline(true)
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .address(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("e.g. 100".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Address".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .value_type(Widget {
                state: SelectionStateBuilder::default()
                    .focused(false)
                    .values(vec![ValueType::Number, ValueType::Text])
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(("Type", HorizontalAlignment::Right).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
                    .build()
                    .unwrap(),
            })
            .number_format(Widget {
                state: SelectionStateBuilder::default()
                    .focused(false)
                    .values(vec![
                        Format(RegisterFormat::U8((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::U16((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::U32((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::U64((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::U128((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::I8((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::I16((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::I32((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::I64((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::I128((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::F32((RegisterEndian::Big, Resolution(1.0)))),
                        Format(RegisterFormat::F64((RegisterEndian::Big, Resolution(1.0)))),
                    ])
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(("Format", HorizontalAlignment::Left).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
                    .build()
                    .unwrap(),
            })
            .number_endian(Widget {
                state: SelectionStateBuilder::default()
                    .focused(false)
                    .values(vec![
                        Endian(RegisterEndian::Big),
                        Endian(RegisterEndian::Little),
                    ])
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(("Endian", HorizontalAlignment::Center).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
                    .build()
                    .unwrap(),
            })
            .number_resolution(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .input("1.0".to_string())
                    .cursor(3)
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(("Resolution", HorizontalAlignment::Right).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .text_alignment(Widget {
                state: SelectionStateBuilder::default()
                    .focused(false)
                    .values(vec![
                        Alignment(TextAlignment::Right),
                        Alignment(TextAlignment::Left),
                    ])
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Alignment".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
                    .build()
                    .unwrap(),
            })
            .text_width(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("1".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(("Width", HorizontalAlignment::Right).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .value(Widget {
                state: SelectionStateBuilder::<V>::default()
                    .focused(true)
                    .values(values)
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Value".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
                    .build()
                    .unwrap(),
            })
            .add_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("ADD".to_string())
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 0,
                    })
                    .style(button_style.clone())
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .unwrap(),
            })
            .delete_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("DEL".to_string())
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 0,
                    })
                    .style(button_style.clone())
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .unwrap(),
            })
            .update_script(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("-- Lua update script (optional)".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Lua Update".into()))
                    .multiline(true)
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .error(Widget {
                state: "".to_string(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(error_style.clone())
                    .build()
                    .unwrap(),
            })
            .success(Widget {
                state: "Everything is fine.".to_string(),
                widget: TextBuilder::default()
                    .title(Some("Success".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(success_style.clone())
                    .build()
                    .unwrap(),
            })
            .keybinds([
                Widget {
                    state: "<Tab>: next | <Space>: press button".to_string(),
                    widget: TextBuilder::default()
                        .margin(Margin {
                            vertical: 0,
                            horizontal: 1,
                        })
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .style(text_style.clone())
                        .build()
                        .unwrap(),
                },
                Widget {
                    state: "<Esc>: cancel | <Enter>: confirm".to_string(),
                    widget: TextBuilder::default()
                        .margin(Margin {
                            vertical: 0,
                            horizontal: 1,
                        })
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .style(text_style.clone())
                        .build()
                        .unwrap(),
                },
            ])
            .focus(EditSelectionDialogFocus::Value)
            .build()
            .unwrap()
    }
}

impl EditSelectionDialog<NamedValue> {
    /// Build the dialog pre-filled from an existing register, its named values, and current value.
    /// `raw_value` is the hex memory string (e.g. `[000a]`) used for accurate integer matching.
    pub fn from_register(
        name: &str,
        comment: &str,
        register: &Register,
        named_values: Vec<NamedValue>,
        current_value: &str,
        raw_value: &str,
        update: Option<&str>,
    ) -> Self {
        let mut dialog = Self::new(named_values.clone());
        set_input(&mut dialog.label, name);
        set_input(&mut dialog.description, comment);
        if let Some(script) = update {
            set_input(&mut dialog.update_script, script);
        }
        dialog.label.state.set_focused(false);
        dialog.value.state.set_focused(true);
        dialog.focus = EditSelectionDialogFocus::Value;
        if let Address::Fixed(addr) = register.address() {
            set_input(&mut dialog.address, &addr.to_string());
        }
        dialog.base_slave_id = *register.slave_id();
        dialog.base_access = register.access().clone();
        dialog.base_kind = register.kind().clone();

        match register.format() {
            RegisterFormat::Ascii((align, width)) => {
                dialog.value_type.state.set_selection(1);
                dialog
                    .text_alignment
                    .state
                    .set_selection(alignment_index(align));
                set_input(&mut dialog.text_width, &width.0.to_string());
            }
            numeric => {
                let (endian, resolution) = numeric_parts(numeric);
                dialog.value_type.state.set_selection(0);
                dialog
                    .number_format
                    .state
                    .set_selection(format_index(numeric));
                dialog
                    .number_endian
                    .state
                    .set_selection(endian_index(&endian));
                set_input(&mut dialog.number_resolution, &resolution.0.to_string());
            }
        }

        // Pre-select the matching named value. Prefer raw memory words (reliable for all
        // formats/resolutions), fall back to parsing the decoded string as i64.
        let raw_int = parse_raw_value(raw_value);
        if let Some(current) = raw_int.or_else(|| current_value.trim().parse::<i64>().ok()) {
            if let Some(idx) = named_values.iter().position(|nv| nv.value == current) {
                dialog.value.state.set_selection(idx);
            }
        }

        dialog
    }

    /// Validate and produce the edited register metadata + the selected named value to write.
    pub fn apply(&self) -> Result<EditedRegister, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let comment = self.description.state.input().trim().to_string();
        let addr = self
            .address
            .state
            .input()
            .trim()
            .parse::<u16>()
            .map_err(|_| "Address must be a number.".to_string())?;

        let format = match self.value_type.state.get_value() {
            ValueType::Number => {
                let selected = self.number_format.state.get_value();
                let endian = self.number_endian.state.get_value().0;
                let resolution = Resolution(
                    self.number_resolution
                        .state
                        .input()
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "Resolution must be a number.".to_string())?,
                );
                with_endian_resolution(&selected.0, endian, resolution)
            }
            ValueType::Text => {
                let alignment = self.text_alignment.state.get_value().0;
                let width = self
                    .text_width
                    .state
                    .input()
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Width must be a number.".to_string())?;
                RegisterFormat::Ascii((alignment, Width(width)))
            }
        };

        let register = RegisterBuilder::default()
            .slave_id(self.base_slave_id)
            .access(self.base_access.clone())
            .kind(self.base_kind.clone())
            .address(Address::Fixed(addr))
            .format(format)
            .build()
            .expect("all register fields are set");

        let named_values = self.value.state.values().clone();
        let value = if named_values.is_empty() {
            None
        } else {
            Some(self.value.state.get_value().value.to_string())
        };
        let update_script = self.update_script.state.input().trim().to_string();
        let update = Some(if update_script.is_empty() {
            String::new()
        } else {
            update_script
        });

        Ok(EditedRegister {
            name,
            comment,
            register,
            value,
            named_values: Some(named_values),
            update,
        })
    }

    pub fn has_sub_dialog(&self) -> bool {
        self.add_dialog.is_some()
    }

    pub fn open_add_dialog(&mut self) {
        self.add_dialog = Some(AddNamedValueDialog::new());
    }

    pub fn close_add_dialog(&mut self) {
        self.add_dialog = None;
    }

    pub fn confirm_add_dialog(&mut self) {
        let result = self.add_dialog.as_ref().map(|d| d.apply());
        match result {
            Some(Ok(nv)) => {
                self.value.state.values_mut().push(nv);
                let idx = self.value.state.values().len() - 1;
                self.value.state.set_selection(idx);
                self.add_dialog = None;
            }
            Some(Err(e)) => {
                if let Some(d) = self.add_dialog.as_mut() {
                    d.error.state = e;
                }
            }
            None => {}
        }
    }

    pub fn add_dialog_focus_next(&mut self) {
        if let Some(d) = self.add_dialog.as_mut() {
            d.focus_next();
        }
    }

    pub fn add_dialog_focus_previous(&mut self) {
        if let Some(d) = self.add_dialog.as_mut() {
            d.focus_previous();
        }
    }

    pub fn add_dialog_handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        if let Some(d) = self.add_dialog.as_mut() {
            let _ = d.handle_events(modifiers, code);
        }
    }

    pub fn handle_space(&mut self) {
        match self.focus {
            EditSelectionDialogFocus::AddButton => self.open_add_dialog(),
            EditSelectionDialogFocus::DeleteButton => self.delete_selected(),
            _ => {}
        }
    }

    pub fn delete_selected(&mut self) {
        let idx = self.value.state.selection();
        let vals = self.value.state.values_mut();
        if !vals.is_empty() {
            vals.remove(idx);
            if !vals.is_empty() {
                let new_idx = if idx >= vals.len() {
                    vals.len() - 1
                } else {
                    idx
                };
                self.value.state.set_selection(new_idx);
            } else {
                self.value.state.set_selection(0);
            }
        }
    }
}
