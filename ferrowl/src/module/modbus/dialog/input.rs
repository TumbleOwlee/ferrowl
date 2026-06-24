//! Free-text register edit dialog: every register property as an input field.

use super::{AccessOption, Alignment, Endian, Format, KindOption, ValueType, parse_address};
use crate::config::device::{NamedValue, Scalar};
use crate::dialog::NonEmpty;
use derive_builder::Builder;
use ferrowl_codec::format::{
    Alignment as TextAlignment, BitField, Endian as RegisterEndian, Format as RegisterFormat,
    Resolution, Width,
};
use ferrowl_codec::{Access, Address, Kind, Register, RegisterBuilder, encode};
use ferrowl_ui::COLOR_SCHEME;
use ferrowl_ui::{
    Border,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, GetValue, InputField,
        InputFieldBuilder, Selection, SelectionBuilder, Text, TextBuilder, Validate,
        ValidateResult, Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct EditInputDialog {
    // Label for the register
    #[focus]
    pub label: Widget<InputFieldState, InputField<NonEmpty>>,
    // Description for the register
    #[focus]
    pub description: Widget<InputFieldState, InputField<String>>,
    // Slave ID for this register
    #[focus]
    pub slave_id: Widget<InputFieldState, InputField<u8>>,
    // Address of the start register
    #[focus]
    pub address: Widget<InputFieldState, InputField<crate::dialog::Address>>,
    // Register kind selection (HoldingRegister, Coil, etc.)
    #[focus]
    pub kind: Widget<SelectionState<KindOption>, Selection<KindOption>>,
    // Access selection (ReadOnly / WriteOnly / ReadWrite)
    #[focus]
    pub access: Widget<SelectionState<AccessOption>, Selection<AccessOption>>,
    // Type selection (hidden for boolean kinds)
    #[focus(when = { !self.is_boolean_kind() })]
    pub value_type: Widget<SelectionState<ValueType>, Selection<ValueType>>,
    // Static "Boolean" label shown instead of Type selector for Coil/DiscreteInput
    pub boolean_type: Widget<String, Text>,
    // Number format selection
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number })]
    pub number_format: Widget<SelectionState<Format>, Selection<Format>>,
    // Number endianess selection
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number })]
    pub number_endian: Widget<SelectionState<Endian>, Selection<Endian>>,
    // Number resolution input
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number })]
    pub number_resolution: Widget<InputFieldState, InputField<f64>>,
    // Bit-field mask input (integer formats only)
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number && is_integer_format(&self.number_format.get_value().0) })]
    pub number_bitmask: Widget<InputFieldState, InputField<crate::dialog::Bitmask>>,
    // Text alignment selection
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Text })]
    pub text_alignment: Widget<SelectionState<Alignment>, Selection<Alignment>>,
    // Text length input
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Text })]
    pub text_width: Widget<InputFieldState, InputField<usize>>,
    // Value input
    #[focus]
    pub value: Widget<InputFieldState, InputField<String>>,
    // Default value stored in the device config and applied on startup
    #[focus]
    pub default_value: Widget<InputFieldState, InputField<String>>,
    // Button to add a predefined named value
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
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
    // Optional sub-dialog for adding a new named value entry.
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
    // Named values accumulated via the ADD button in this session.
    #[builder(default)]
    pub pending_named_values: Vec<NamedValue>,
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

/// The result of confirming the edit dialog: updated register metadata + an optional value to
/// write.
#[derive(Debug, Clone)]
pub struct EditedRegister {
    pub name: String,
    pub description: String,
    pub register: Register,
    pub value: Option<String>,
    /// Updated named-value list from EditSelectionDialog; None means unchanged.
    pub named_values: Option<Vec<crate::config::device::NamedValue>>,
    /// Lua update script content; None means unchanged (field not shown).
    pub update: Option<String>,
    /// Default value to store in the device config (applied on startup). None = no default.
    pub default: Option<Scalar>,
}

impl EditInputDialog {
    fn is_boolean_kind(&self) -> bool {
        matches!(
            self.kind.state.get_value().0,
            Kind::Coil | Kind::DiscreteInput
        )
    }

    fn validate(&self) -> Result<(), String> {
        if let ValidateResult::Error(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        } else if let ValidateResult::Error(e) = u8::validate(self.slave_id.state.input()) {
            return Err(format!("Slave ID: {e}"));
        } else if let Err(e) = parse_address(self.address.state.input()) {
            return Err(format!("Address: {e}"));
        }

        if !self.is_boolean_kind() {
            match self.value_type.state.values()[self.value_type.state.selection()] {
                ValueType::Number => {
                    if let ValidateResult::Error(e) =
                        f64::validate(self.number_resolution.state.input())
                    {
                        return Err(format!("Resolution: {e}"));
                    }
                    let format =
                        &self.number_format.state.values()[self.number_format.state.selection()].0;
                    if is_integer_format(format)
                        && let Err(e) = parse_bitmask(self.number_bitmask.state.input())
                    {
                        return Err(format!("Bitmask: {e}"));
                    }
                    let v = self.value.state.input();
                    let s = v.trim();
                    if let Err(e) = encode(format, s) {
                        return Err(format!("Value: cannot convert '{s}' to number [{e}]"));
                    }
                    let v = self.default_value.state.input();
                    let s = v.trim();
                    if !s.is_empty()
                        && let Err(e) = encode(format, s)
                    {
                        return Err(format!("Value: cannot convert '{s}' to number [{e}]"));
                    }
                }
                ValueType::Text => {
                    if let ValidateResult::Error(e) = usize::validate(self.text_width.state.input())
                    {
                        return Err(format!("Width: {e}"));
                    }
                }
            }
        }
        Ok(())
    }

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

    pub fn new() -> Self {
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

        EditInputDialogBuilder::default()
            .label(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(true)
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
                    .placeholder(Some("100 or 'virtual'".to_string()))
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
                        KindOption(Kind::Coil),
                        KindOption(Kind::DiscreteInput),
                        KindOption(Kind::HoldingRegister),
                        KindOption(Kind::InputRegister),
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
                            AccessOption(Access::ReadOnly),
                            AccessOption(Access::WriteOnly),
                            AccessOption(Access::ReadWrite),
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
            .boolean_type(Widget {
                state: "Boolean".to_string(),
                widget: TextBuilder::default()
                    .title(Some(("Type", HorizontalAlignment::Right).into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(text_style.clone())
                    .build()
                    .unwrap(),
            })
            .number_format(Widget {
                state: SelectionStateBuilder::default()
                    .focused(false)
                    .values(vec![
                        Format(RegisterFormat::U8((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::U16((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::U32((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::U64((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::U128((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::I8((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::I16((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::I32((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::I64((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
                        Format(RegisterFormat::I128((
                            RegisterEndian::Big,
                            Resolution(1.0),
                            BitField::default(),
                        ))),
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
                    .title(Some(("Resolution", HorizontalAlignment::Center).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .number_bitmask(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("0xFFFF".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(("Bitmask", HorizontalAlignment::Right).into()))
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
                        Alignment(TextAlignment::Left),
                        Alignment(TextAlignment::Right),
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
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("Enter value...".to_string()))
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
            .default_value(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("Default value (applied on startup)...".to_string()))
                    .build()
                    .unwrap(),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Default".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .unwrap(),
            })
            .add_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("ADD PREDEFINED".to_string())
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(ButtonStyle::default())
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
                    .style(ButtonStyle::default())
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .build()
                    .unwrap(),
            })
            .confirm_button(Widget {
                state: ButtonStateBuilder::default()
                    .focused(false)
                    .label("CONFIRM".to_string())
                    .disabled(false)
                    .build()
                    .unwrap(),
                widget: ButtonBuilder::default()
                    .border_margin(Margin::new(1, 0))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(ButtonStyle::default())
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
                    .multiline(true)
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
                    state: "<Space>: press button | <C-f>: fill value | <Tab>: next".to_string(),
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
            .focus(EditInputDialogFocus::Label)
            .build()
            .unwrap()
    }

    /// Build the dialog pre-filled from an existing register and its current value. Focus
    /// starts on the value field so editing the value (the common case) works immediately.
    pub fn from_register(
        name: &str,
        description: &str,
        register: &Register,
        value: &str,
        update: Option<&str>,
        default: Option<&Scalar>,
    ) -> Self {
        let mut dialog = Self::new();
        dialog.deletable = true;
        set_input(&mut dialog.label, name);
        set_input(&mut dialog.description, description);
        if let Some(script) = update {
            dialog.update_script.state.set_content(script);
        }
        if let Some(def) = default {
            set_input(&mut dialog.default_value, &def.to_string());
        }
        // Pre-populate the value field so the user can edit or clear it directly.
        let is_ascii = matches!(register.format(), RegisterFormat::Ascii(_));
        if is_ascii {
            let value: String = if matches!(
                register.format(),
                RegisterFormat::Ascii((ferrowl_codec::Alignment::Left, _))
            ) {
                let value: String = value
                    .chars()
                    .rev()
                    .skip_while(|c| !c.is_ascii_graphic())
                    .map(|c| if !c.is_ascii_graphic() { ' ' } else { c })
                    .collect();
                value.chars().rev().collect()
            } else {
                value
                    .chars()
                    .skip_while(|c| !c.is_ascii_graphic())
                    .map(|c| if !c.is_ascii_graphic() { ' ' } else { c })
                    .collect()
            };
            set_input(&mut dialog.value, &value);
        } else {
            set_input(&mut dialog.value, value);
        }
        dialog.label.state.set_focused(false);
        dialog.value.state.set_focused(true);
        dialog.focus = EditInputDialogFocus::Value;
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
                let (endian, resolution, bitfield) = numeric_parts(numeric);
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
                // Show the mask only when it actually selects a sub-field.
                if !bitfield.is_full() {
                    set_input(
                        &mut dialog.number_bitmask,
                        &format!("0x{:X}", bitfield.mask),
                    );
                }
            }
        }
        dialog
    }

    /// Validate and produce the edited register metadata + optional value to write.
    pub fn apply(&self) -> Result<EditedRegister, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let description = self.description.state.input().trim().to_string();
        let address = parse_address(self.address.state.input())?;

        let format = if self.is_boolean_kind() {
            RegisterFormat::U16((RegisterEndian::Big, Resolution(1.0), BitField::default()))
        } else {
            match self.value_type.state.get_value() {
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
                    // Bitmask applies to integer formats only; floats ignore it.
                    let bitfield = if is_integer_format(&selected.0) {
                        parse_bitmask(self.number_bitmask.state.input())
                            .map_err(|e| format!("Bitmask {e}."))?
                    } else {
                        BitField::default()
                    };
                    with_numeric_parts(&selected.0, endian, resolution, bitfield)
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
            }
        };
        let is_ascii = matches!(format, RegisterFormat::Ascii(_));

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

        let input = self.value.state.input().to_string();
        let value = if is_ascii || !input.trim().is_empty() {
            Some(input)
        } else {
            None
        };
        let update_script = self.update_script.state.content().trim().to_string();
        let update = Some(if update_script.is_empty() {
            String::new()
        } else {
            update_script
        });

        let named_values = if self.pending_named_values.is_empty() {
            None
        } else {
            Some(self.pending_named_values.clone())
        };

        let default = {
            let s = self.default_value.state.input().trim();
            if s.is_empty() {
                None
            } else {
                Some(Scalar::from_input(s))
            }
        };

        Ok(EditedRegister {
            name,
            description,
            register,
            value,
            named_values,
            update,
            default,
        })
    }

    pub fn handle_space(&mut self) {
        match self.focus {
            EditInputDialogFocus::AddButton => self.open_add_dialog(),
            EditInputDialogFocus::DeleteRegisterButton => self.open_confirm_delete(),
            _ => {
                self.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
            }
        }
    }

    pub fn is_delete_register_button_focused(&self) -> bool {
        matches!(self.focus, EditInputDialogFocus::DeleteRegisterButton)
    }

    pub fn is_update_script_focused(&self) -> bool {
        matches!(self.focus, EditInputDialogFocus::UpdateScript)
    }

    pub fn is_confirm_button_focused(&self) -> bool {
        matches!(self.focus, EditInputDialogFocus::ConfirmButton)
    }

    /// Convert this dialog into an EditSelectionDialog, preserving shared field state.
    /// Called when the first named value is added and the dialog should switch to selection mode.
    pub fn to_edit_selection_dialog(
        &self,
    ) -> super::selection::EditSelectionDialog<crate::config::device::NamedValue> {
        use crate::config::device::{NamedValue, Scalar};
        let values = self.pending_named_values.clone();
        let mut d = super::selection::EditSelectionDialog::new(values.clone());
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
        d.number_bitmask.state = self.number_bitmask.state.clone();
        d.text_alignment.state = self.text_alignment.state.clone();
        d.text_width.state = self.text_width.state.clone();
        d.update_script.state = self.update_script.state.clone();

        // Set up default selection with sentinel and try to match prior text default.
        let mut default_vals = vec![NamedValue {
            name: "(no default)".to_string(),
            value: Scalar::Text("".into()),
        }];
        default_vals.extend_from_slice(&values);
        *d.default_value.state.values_mut() = default_vals;
        let default_text = self.default_value.state.input().trim().to_string();
        if !default_text.is_empty()
            && let Some(idx) = values
                .iter()
                .position(|nv| nv.value.to_string() == default_text)
        {
            d.default_value.state.set_selection(idx + 1);
        }
        d
    }
}

impl SubDialogs for EditInputDialog {
    fn add_dialog_opt(&self) -> Option<&AddNamedValueDialog> {
        self.add_dialog.as_ref()
    }

    fn add_dialog_slot(&mut self) -> &mut Option<AddNamedValueDialog> {
        &mut self.add_dialog
    }

    fn confirm_delete_opt(&self) -> Option<&ConfirmDeleteDialog> {
        self.confirm_delete.as_ref()
    }

    fn confirm_delete_slot(&mut self) -> &mut Option<ConfirmDeleteDialog> {
        &mut self.confirm_delete
    }

    fn name_error_slot(&mut self) -> &mut Option<String> {
        &mut self.name_error
    }

    fn register_label(&self) -> String {
        self.label.state.input().trim().to_string()
    }

    fn accept_named_value(&mut self, nv: NamedValue) {
        self.pending_named_values.push(nv);
    }
}

impl super::RegisterDialog for EditInputDialog {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.render(area, buf)
    }
    fn focus_next(&mut self) {
        self.focus_next()
    }
    fn focus_previous(&mut self) {
        self.focus_previous()
    }
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let _ = HandleEvents::handle_events(self, modifiers, code);
    }
    fn handle_space(&mut self) {
        self.handle_space()
    }
    fn is_update_script_focused(&self) -> bool {
        self.is_update_script_focused()
    }
    fn is_confirm_button_focused(&self) -> bool {
        self.is_confirm_button_focused()
    }
    fn is_delete_register_button_focused(&self) -> bool {
        self.is_delete_register_button_focused()
    }
    fn apply(&self) -> Result<EditedRegister, String> {
        self.apply()
    }
}

use super::{
    AddNamedValueDialog, ConfirmDeleteDialog, SubDialogs, access_index, alignment_index,
    endian_index, format_index, is_integer_format, kind_index, numeric_parts, parse_bitmask,
    set_input, with_numeric_parts,
};
use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;

#[cfg(test)]
mod apply_tests {
    //! Characterization tests for the `from_register` → `apply` round-trip: editing an existing
    //! register and confirming must reproduce its metadata.
    use super::EditInputDialog;
    use ferrowl_codec::format::{
        Alignment as TextAlignment, BitField, Endian as RegisterEndian, Format as RegisterFormat,
        Resolution, Width,
    };
    use ferrowl_codec::{Access, Address, Kind, Register, RegisterBuilder};

    fn reg(kind: Kind, access: Access, address: Address, slave: u8, format: RegisterFormat) -> Register {
        RegisterBuilder::default()
            .slave_id(slave)
            .access(access)
            .kind(kind)
            .address(address)
            .format(format)
            .build()
            .unwrap()
    }

    #[test]
    fn ut_numeric_register_round_trips_through_apply() {
        let original = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(100),
            7,
            RegisterFormat::U32((RegisterEndian::Big, Resolution(1.0), BitField::default())),
        );
        let edited = EditInputDialog::from_register("temp", "a sensor", &original, "42", None, None)
            .apply()
            .expect("valid register should apply");

        assert_eq!(edited.name, "temp");
        assert_eq!(edited.description, "a sensor");
        assert_eq!(*edited.register.slave_id(), 7);
        assert_eq!(*edited.register.kind(), Kind::HoldingRegister);
        assert_eq!(*edited.register.access(), Access::ReadWrite);
        assert_eq!(*edited.register.address(), Address::Fixed(100));
        assert_eq!(edited.register.format(), original.format());
        assert_eq!(edited.value.as_deref(), Some("42"));
    }

    #[test]
    fn ut_virtual_address_and_read_only_round_trip() {
        let original = reg(
            Kind::InputRegister,
            Access::ReadOnly,
            Address::Virtual,
            1,
            RegisterFormat::U16((RegisterEndian::Little, Resolution(0.5), BitField::default())),
        );
        let edited = EditInputDialog::from_register("v", "", &original, "3", None, None)
            .apply()
            .expect("valid register should apply");

        assert_eq!(*edited.register.address(), Address::Virtual);
        assert_eq!(*edited.register.access(), Access::ReadOnly);
        assert_eq!(edited.register.format(), original.format());
    }

    #[test]
    fn ut_non_full_bitmask_round_trips() {
        let original = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(5),
            1,
            RegisterFormat::U16((RegisterEndian::Big, Resolution(1.0), BitField { mask: 0xFF00 })),
        );
        let edited = EditInputDialog::from_register("masked", "", &original, "0", None, None)
            .apply()
            .expect("valid register should apply");
        assert_eq!(edited.register.format(), original.format());
    }

    #[test]
    fn ut_ascii_register_round_trips_format() {
        let original = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::Ascii((TextAlignment::Left, Width(4))),
        );
        let edited = EditInputDialog::from_register("label", "", &original, "AB", None, None)
            .apply()
            .expect("valid register should apply");
        assert_eq!(edited.register.format(), original.format());
    }

    #[test]
    fn ut_boolean_kind_forces_default_u16_format() {
        let original = reg(
            Kind::Coil,
            Access::ReadWrite,
            Address::Fixed(1),
            1,
            RegisterFormat::U16((RegisterEndian::Big, Resolution(1.0), BitField::default())),
        );
        let edited = EditInputDialog::from_register("c", "", &original, "1", None, None)
            .apply()
            .expect("valid register should apply");
        assert_eq!(*edited.register.kind(), Kind::Coil);
        // Boolean kinds (Coil/DiscreteInput) always serialize as a default big-endian U16.
        assert_eq!(
            *edited.register.format(),
            RegisterFormat::U16((RegisterEndian::Big, Resolution(1.0), BitField::default()))
        );
    }

    #[test]
    fn ut_empty_add_dialog_does_not_apply() {
        // A freshly opened "Add" dialog has empty fields (no slave id / value), so confirming it
        // must fail validation rather than produce a bogus register.
        assert!(EditInputDialog::new().apply().is_err());
    }
}

#[cfg(test)]
mod focus_tests {
    //! Characterization tests for the `#[derive(Focus)]`-generated event dispatch and focus cycle:
    //! `handle_events` routes a key to the focused pane, and `focus_next`/`focus_previous` cycle
    //! through the focusable panes while skipping `#[focus(when = …)]`-gated ones.
    use super::{EditInputDialog, EditInputDialogFocus};
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_codec::format::{
        BitField, Endian as RegisterEndian, Format as RegisterFormat, Resolution,
    };
    use ferrowl_codec::{Access, Address, Kind, Register, RegisterBuilder};
    use ferrowl_ui::traits::HandleEvents;

    fn numeric_dialog() -> EditInputDialog {
        let register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(RegisterFormat::U32((
                RegisterEndian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();
        // `from_register` focuses the value field and sets the cursor at the end of "4".
        EditInputDialog::from_register("name", "", &register, "4", None, None)
    }

    fn coil_dialog() -> EditInputDialog {
        let register: Register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::Coil)
            .address(Address::Fixed(0))
            .format(RegisterFormat::U16((
                RegisterEndian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();
        EditInputDialog::from_register("c", "", &register, "1", None, None)
    }

    /// Walk a full forward focus cycle, returning every focus state visited (starting state first).
    fn forward_cycle(dialog: &mut EditInputDialog) -> Vec<EditInputDialogFocus> {
        let start = dialog.focus;
        let mut seen = vec![start];
        for _ in 0..64 {
            dialog.focus_next();
            if dialog.focus == start {
                return seen;
            }
            seen.push(dialog.focus);
        }
        panic!("focus_next did not return to the starting pane within 64 steps");
    }

    #[test]
    fn ut_handle_events_types_into_focused_value_field() {
        let mut d = numeric_dialog();
        assert_eq!(d.focus, EditInputDialogFocus::Value);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('2'));
        // The keystroke is routed to the focused value field (cursor was at the end of "4").
        assert_eq!(d.value.state.input(), "42");
        // Other fields are untouched.
        assert_eq!(d.label.state.input(), "name");
    }

    #[test]
    fn ut_handle_events_follows_focus_to_another_pane() {
        let mut d = numeric_dialog();
        d.focus = EditInputDialogFocus::Label;
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('x'));
        // Now the label receives the keystroke; the value field stays at "4".
        assert_eq!(d.label.state.input(), "namex");
        assert_eq!(d.value.state.input(), "4");
    }

    #[test]
    fn ut_focus_cycle_wraps_and_visits_core_panes() {
        let mut d = numeric_dialog();
        let seen = forward_cycle(&mut d);
        // Wrapped back to the starting pane.
        assert_eq!(d.focus, EditInputDialogFocus::Value);
        // Core always-present panes and the editing register's numeric + delete panes are reached.
        for expected in [
            EditInputDialogFocus::Label,
            EditInputDialogFocus::SlaveId,
            EditInputDialogFocus::Address,
            EditInputDialogFocus::Value,
            EditInputDialogFocus::NumberFormat,
            EditInputDialogFocus::ConfirmButton,
            EditInputDialogFocus::DeleteRegisterButton,
        ] {
            assert!(seen.contains(&expected), "cycle missing {expected:?}: {seen:?}");
        }
    }

    #[test]
    fn ut_focus_previous_reverses_focus_next() {
        let mut d = numeric_dialog();
        let start = d.focus;
        d.focus_next();
        assert_ne!(d.focus, start);
        d.focus_previous();
        assert_eq!(d.focus, start);
    }

    #[test]
    fn ut_focus_cycle_skips_gated_number_panes_for_boolean_kind() {
        let mut d = coil_dialog();
        let seen = forward_cycle(&mut d);
        // Coil/DiscreteInput are boolean: the type selector and all numeric/text sub-panes are
        // gated off and must be skipped by the cycle.
        for gated in [
            EditInputDialogFocus::ValueType,
            EditInputDialogFocus::NumberFormat,
            EditInputDialogFocus::NumberEndian,
            EditInputDialogFocus::NumberResolution,
            EditInputDialogFocus::TextAlignment,
            EditInputDialogFocus::TextWidth,
        ] {
            assert!(!seen.contains(&gated), "boolean cycle should skip {gated:?}: {seen:?}");
        }
        // The value field is still reachable.
        assert!(seen.contains(&EditInputDialogFocus::Value));
    }
}
