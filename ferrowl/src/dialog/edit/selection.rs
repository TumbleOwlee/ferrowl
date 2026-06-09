use crate::config::device::{NamedValue, Scalar};
use crate::dialog::EditedRegister;
use crate::dialog::edit::{
    AccessOption, Alignment, ConfirmDeleteDialog, Endian, Format, KindOption, ValueType,
    access_index, alignment_index, endian_index, format_index, kind_index, numeric_parts,
    parse_address, set_input, with_endian_resolution,
};
use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_reg::format::{
    Alignment as TextAlignment, Endian as RegisterEndian, Format as RegisterFormat, Resolution,
    Width,
};
use ferrowl_reg::{Address, Register, RegisterBuilder};
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    traits::HandleEvents,
    traits::ToLabel,
    types::Border,
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, GetValue, InputField,
        InputFieldBuilder, Selection, SelectionBuilder, Text, TextBuilder, Validate, Widget,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

/// Parse a raw memory string like `[00a0 0001]` into an i64 (big-endian word combination).
pub fn parse_raw_value(raw: &str) -> Option<i64> {
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
    pub value: Widget<InputFieldState, InputField<String>>,
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
                    state: "<Esc>: cancel | <Enter>: confirm / newline".to_string(),
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
        if self.value.state.input().trim().is_empty() {
            return Err("Value must not be empty.".to_string());
        }
        Ok(())
    }

    pub fn apply(&self) -> Result<NamedValue, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        // Accept int, float or text; the type is inferred from the input.
        let value = Scalar::from_input(self.value.state.input());
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
    // Slave ID for this register
    #[focus]
    pub slave_id: Widget<InputFieldState, InputField<u8>>,
    // Address of the start register
    #[focus]
    pub address: Widget<InputFieldState, InputField<String>>,
    // Register kind selection (HoldingRegister, Coil, etc.)
    #[focus]
    pub kind: Widget<SelectionState<KindOption>, Selection<KindOption>>,
    // Access selection (ReadOnly / WriteOnly / ReadWrite)
    #[focus]
    pub access: Widget<SelectionState<AccessOption>, Selection<AccessOption>>,
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
    #[focus(when = {!self.value.state.values().is_empty()})]
    pub value: Widget<SelectionState<V>, Selection<V>>,
    // Default value selection (same options as value, plus a leading "no default" sentinel)
    #[focus(when = {!self.value.state.values().is_empty()})]
    pub default_value: Widget<SelectionState<V>, Selection<V>>,
    // Add button
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    // Delete button
    #[focus(when = {!self.value.state.values().is_empty()})]
    pub delete_button: Widget<ButtonState, Button>,
    // Lua simulation script (optional multiline)
    #[focus]
    pub update_script: Widget<CodeInputFieldState, CodeInputField>,
    // Confirm button
    #[focus]
    pub confirm_button: Widget<ButtonState, Button>,
    // Delete-register button (only focusable when editing an existing register)
    #[focus(when = { self.deletable })]
    pub delete_register_button: Widget<ButtonState, Button>,
    // Error display field
    pub error: Widget<String, Text>,
    // Success display field
    pub success: Widget<String, Text>,
    // Keybinds display field
    pub keybinds: [Widget<String, Text>; 2],
    // Optional add-value sub-dialog
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
    // Whether this dialog edits an existing register (enables the delete button).
    #[builder(default)]
    pub deletable: bool,
    // Optional confirmation box guarding register deletion.
    #[builder(default)]
    pub confirm_delete: Option<ConfirmDeleteDialog>,
    // Name-conflict error set by the app at confirm time. Survives the per-frame `validate()`
    // refresh (which can't see other registers) until the user edits the dialog again.
    #[builder(default)]
    pub name_error: Option<String>,
}

impl<V: ToLabel + Clone> EditSelectionDialog<V> {
    fn validate(&self) -> Result<(), String> {
        if let Err(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        } else if let Err(e) = u8::validate(self.slave_id.state.input()) {
            return Err(format!("Slave ID: {e}"));
        } else if let Err(e) = parse_address(self.address.state.input()) {
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
            Constraint::Length(27 + 2 + 2 + 3 + 3 + 3),
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
            Constraint::Length(6),
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

    pub fn new(values: Vec<V>) -> Self {
        let default_values = values.clone();
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
            .slave_id(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("e.g. 1".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Slave ID".into()))
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
            .kind(Widget {
                state: SelectionStateBuilder::default()
                    .focused(false)
                    .values(vec![
                        KindOption(ferrowl_reg::Kind::Coil),
                        KindOption(ferrowl_reg::Kind::DiscreteInput),
                        KindOption(ferrowl_reg::Kind::HoldingRegister),
                        KindOption(ferrowl_reg::Kind::InputRegister),
                    ])
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Kind".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
                    .build()
                    .unwrap(),
            })
            .access(Widget {
                state: {
                    let mut s = SelectionStateBuilder::default()
                        .focused(false)
                        .values(vec![
                            AccessOption(ferrowl_reg::Access::ReadOnly),
                            AccessOption(ferrowl_reg::Access::WriteOnly),
                            AccessOption(ferrowl_reg::Access::ReadWrite),
                        ])
                        .build()
                        .unwrap();
                    s.set_selection(2);
                    s
                },
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Access".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(selection_style.clone())
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
            .default_value(Widget {
                state: SelectionStateBuilder::<V>::default()
                    .focused(false)
                    .values(default_values)
                    .build()
                    .unwrap(),
                widget: SelectionBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Default".into()))
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
                state: CodeInputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("-- Lua update script (optional)".to_string()))
                    .build()
                    .unwrap(),
                widget: CodeInputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Lua Update".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .delete_register_button(Widget {
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
                    .style(button_style.clone())
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .unwrap(),
            })
            .confirm_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("Confirm".to_string())
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
                    state: "<Esc>: cancel | <Enter>: confirm / newline".to_string(),
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
    #[allow(clippy::too_many_arguments)]
    pub fn from_register(
        name: &str,
        description: &str,
        register: &Register,
        named_values: Vec<NamedValue>,
        current_value: &str,
        raw_value: &str,
        update: Option<&str>,
        default: Option<&Scalar>,
    ) -> Self {
        let mut dialog = Self::new(named_values.clone());
        dialog.deletable = true;
        set_input(&mut dialog.label, name);
        set_input(&mut dialog.description, description);
        if let Some(script) = update {
            dialog.update_script.state.set_content(script);
        }
        // Populate default selection: sentinel at index 0, then all named values.
        let mut default_vals = vec![NamedValue {
            name: "(no default)".to_string(),
            value: Scalar::Text("".into()),
        }];
        default_vals.extend_from_slice(&named_values);
        *dialog.default_value.state.values_mut() = default_vals;
        if let Some(def) = default {
            let def_str = def.to_string();
            if let Some(idx) = named_values
                .iter()
                .position(|nv| nv.value.to_string() == def_str)
            {
                dialog.default_value.state.set_selection(idx + 1);
            }
        }
        dialog.label.state.set_focused(false);
        dialog.value.state.set_focused(true);
        dialog.focus = EditSelectionDialogFocus::Value;
        match register.address() {
            Address::Fixed(addr) => set_input(&mut dialog.address, &addr.to_string()),
            Address::Virtual => set_input(&mut dialog.address, "virtual"),
        }
        set_input(&mut dialog.slave_id, &register.slave_id().to_string());
        dialog
            .access
            .state
            .set_selection(access_index(register.access()));
        dialog.kind.state.set_selection(kind_index(register.kind()));

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

        // Pre-select the matching named value. Integer values match the raw memory words (reliable
        // across formats/resolutions); any value type also matches the decoded display string.
        let raw_int = parse_raw_value(raw_value);
        let current = current_value.trim();
        if let Some(idx) = named_values.iter().position(|nv| match &nv.value {
            Scalar::Int(v) => raw_int == Some(*v) || current == v.to_string(),
            other => current == other.to_string(),
        }) {
            dialog.value.state.set_selection(idx);
        }

        dialog
    }

    /// Validate and produce the edited register metadata + the selected named value to write.
    pub fn apply(&self) -> Result<EditedRegister, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let description = self.description.state.input().trim().to_string();
        let address = parse_address(self.address.state.input())?;

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

        let slave_id = self
            .slave_id
            .state
            .input()
            .trim()
            .parse::<u8>()
            .map_err(|_| "Slave ID must be 0–255.".to_string())?;

        let register = RegisterBuilder::default()
            .slave_id(slave_id)
            .access(self.access.state.get_value().0.clone())
            .kind(self.kind.state.get_value().0)
            .address(address)
            .format(format)
            .build()
            .expect("all register fields are set");

        let named_values = self.value.state.values().clone();
        let value = if named_values.is_empty() {
            None
        } else {
            Some(self.value.state.get_value().value.to_string())
        };
        let update_script = self.update_script.state.content().trim().to_string();
        let update = Some(if update_script.is_empty() {
            String::new()
        } else {
            update_script
        });

        let default = {
            let sel = self.default_value.state.selection();
            let vals = self.default_value.state.values();
            if sel == 0 || vals.len() <= 1 {
                None
            } else {
                Some(vals[sel].value.clone())
            }
        };

        Ok(EditedRegister {
            name,
            description,
            register,
            value,
            named_values: Some(named_values),
            update,
            default,
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
                self.value.state.values_mut().push(nv.clone());
                let idx = self.value.state.values().len() - 1;
                self.value.state.set_selection(idx);
                // Keep default selection in sync: append after the sentinel.
                self.default_value.state.values_mut().push(nv);
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
            EditSelectionDialogFocus::DeleteRegisterButton => self.open_confirm_delete(),
            _ => {
                self.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
            }
        }
    }

    pub fn is_delete_register_button_focused(&self) -> bool {
        matches!(self.focus, EditSelectionDialogFocus::DeleteRegisterButton)
    }

    pub fn set_name_error(&mut self, msg: String) {
        self.name_error = Some(msg);
    }

    pub fn clear_name_error(&mut self) {
        self.name_error = None;
    }

    pub fn has_confirm_delete(&self) -> bool {
        self.confirm_delete.is_some()
    }

    pub fn open_confirm_delete(&mut self) {
        let name = self.label.state.input().trim().to_string();
        self.confirm_delete = Some(ConfirmDeleteDialog::new(&name));
    }

    pub fn close_confirm_delete(&mut self) {
        self.confirm_delete = None;
    }

    pub fn confirm_delete_focus_next(&mut self) {
        if let Some(d) = self.confirm_delete.as_mut() {
            d.focus_next();
        }
    }

    pub fn confirm_delete_focus_previous(&mut self) {
        if let Some(d) = self.confirm_delete.as_mut() {
            d.focus_previous();
        }
    }

    pub fn confirm_delete_is_confirmed(&self) -> bool {
        self.confirm_delete
            .as_ref()
            .map(|d| d.is_confirm_focused())
            .unwrap_or(false)
    }

    pub fn is_update_script_focused(&self) -> bool {
        matches!(self.focus, EditSelectionDialogFocus::UpdateScript)
    }

    pub fn is_confirm_button_focused(&self) -> bool {
        matches!(self.focus, EditSelectionDialogFocus::ConfirmButton)
    }

    /// Convert this dialog into an EditInputDialog, preserving all shared field state.
    /// Called when all named values are removed and the dialog should switch to free-text mode.
    pub fn to_edit_input_dialog(&self) -> super::input::EditInputDialog {
        let mut d = super::input::EditInputDialog::new();
        d.deletable = self.deletable;
        d.label.state = self.label.state.clone();
        d.description.state = self.description.state.clone();
        d.slave_id.state = self.slave_id.state.clone();
        d.address.state = self.address.state.clone();
        d.kind.state = self.kind.state.clone();
        d.access.state = self.access.state.clone();
        d.value_type.state = self.value_type.state.clone();
        d.number_format.state = self.number_format.state.clone();
        d.number_endian.state = self.number_endian.state.clone();
        d.number_resolution.state = self.number_resolution.state.clone();
        d.text_alignment.state = self.text_alignment.state.clone();
        d.text_width.state = self.text_width.state.clone();
        d.update_script.state = self.update_script.state.clone();
        // Convert selected default → text (skip sentinel at index 0).
        let sel = self.default_value.state.selection();
        if sel > 0
            && let Some(nv) = self.default_value.state.values().get(sel)
        {
            set_input(&mut d.default_value, &nv.value.to_string());
        }
        d
    }

    pub fn delete_selected(&mut self) {
        let idx = self.value.state.selection();
        let vals = self.value.state.values_mut();
        let mut is_empty = vals.is_empty();
        if !vals.is_empty() {
            vals.remove(idx);
            is_empty = vals.is_empty();
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

            // Sync default selection: idx+1 because sentinel sits at position 0.
            let default_idx = idx + 1;
            let default_vals = self.default_value.state.values_mut();
            if default_idx < default_vals.len() {
                default_vals.remove(default_idx);
                let default_sel = self.default_value.state.selection();
                if default_sel >= default_idx {
                    // If exactly the deleted entry was selected, reset to "no default";
                    // otherwise shift the selection down to stay on the same item.
                    let new_sel = if default_sel == default_idx {
                        0
                    } else {
                        default_sel - 1
                    };
                    self.default_value.state.set_selection(new_sel);
                }
            }
        }

        if is_empty {
            self.focus_previous();
        }
    }
}
