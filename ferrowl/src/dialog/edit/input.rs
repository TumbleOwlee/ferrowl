use crate::config::device::NamedValue;
use crate::dialog::edit::{
    AccessOption, Alignment, Endian, Format, KindOption, ValueType, parse_address,
};
use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_reg::format::{
    Alignment as TextAlignment, Endian as RegisterEndian, Format as RegisterFormat, Resolution,
    Width,
};
use ferrowl_reg::{Access, Address, Kind, Register, RegisterBuilder};
use ferrowl_ui::COLOR_SCHEME;
use ferrowl_ui::{
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
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

#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct EditInputDialog {
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
    // Text alignment selection
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Text })]
    pub text_alignment: Widget<SelectionState<Alignment>, Selection<Alignment>>,
    // Text length input
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Text })]
    pub text_width: Widget<InputFieldState, InputField<usize>>,
    // Value input
    #[focus]
    pub value: Widget<InputFieldState, InputField<String>>,
    // Button to add a predefined named value
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    // Lua simulation script (optional multiline)
    #[focus]
    pub update_script: Widget<CodeInputFieldState, CodeInputField>,
    // Confirm button
    #[focus]
    pub confirm_button: Widget<ButtonState, Button>,
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
}

/// The result of confirming the edit dialog: updated register metadata + an optional value to
/// write.
#[derive(Debug, Clone)]
pub struct EditedRegister {
    pub name: String,
    pub comment: String,
    pub register: Register,
    pub value: Option<String>,
    /// Updated named-value list from EditSelectionDialog; None means unchanged.
    pub named_values: Option<Vec<crate::config::device::NamedValue>>,
    /// Lua update script content; None means unchanged (field not shown).
    pub update: Option<String>,
}

impl EditInputDialog {
    fn is_boolean_kind(&self) -> bool {
        matches!(
            self.kind.state.get_value().0,
            Kind::Coil | Kind::DiscreteInput
        )
    }

    fn validate(&self) -> Result<(), String> {
        if let Err(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        } else if let Err(e) = u8::validate(self.slave_id.state.input()) {
            return Err(format!("Slave ID: {e}"));
        } else if let Err(e) = parse_address(self.address.state.input()) {
            return Err(format!("Address: {e}"));
        }

        if !self.is_boolean_kind() {
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
        }
        Ok(())
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Show error
        match self.validate() {
            Ok(_) => {
                self.error.state.clear();
            }
            Err(e) => {
                self.error.state = e;
            }
        }

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(70), Constraint::Min(1)])
                .areas(area);

        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(40),
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
            .title("Edit");
        let dialog_box = vertical_layout[1];
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

        StatefulWidget::render(
            &self.confirm_button.widget,
            vertical_layout[vertical_index],
            buf,
            &mut self.confirm_button.state,
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

        if let Some(d) = self.add_dialog.as_mut() {
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
        comment: &str,
        register: &Register,
        value: &str,
        update: Option<&str>,
    ) -> Self {
        let mut dialog = Self::new();
        set_input(&mut dialog.label, name);
        set_input(&mut dialog.description, comment);
        if let Some(script) = update {
            dialog.update_script.state.set_content(script);
        }
        // Show the current value as a placeholder; <C-f> fills it in when the field is empty.
        if !value.is_empty() {
            dialog.value.state.set_placeholder(Some(value.to_string()));
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
        dialog
    }

    /// Validate and produce the edited register metadata + optional value to write.
    pub fn apply(&self) -> Result<EditedRegister, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let comment = self.description.state.input().trim().to_string();
        let address = parse_address(self.address.state.input())?;

        let format = if self.is_boolean_kind() {
            RegisterFormat::U16((RegisterEndian::Big, Resolution(1.0)))
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

        let value = {
            let v = self.value.state.input().trim();
            if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            }
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

        Ok(EditedRegister {
            name,
            comment,
            register,
            value,
            named_values,
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
                self.pending_named_values.push(nv);
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
        if let EditInputDialogFocus::AddButton = self.focus {
            self.open_add_dialog();
        } else {
            self.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        }
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
        let values = self.pending_named_values.clone();
        let mut d = super::selection::EditSelectionDialog::new(values);
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
        d
    }
}

use super::{
    AddNamedValueDialog, access_index, alignment_index, endian_index, format_index, kind_index,
    numeric_parts, set_input, with_endian_resolution,
};
use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;
