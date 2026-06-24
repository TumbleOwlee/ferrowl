//! Construction of the free-text register edit dialog widget tree.

use super::{
    AccessOption, Alignment, EditInputDialog, EditInputDialogBuilder, EditInputDialogFocus, Endian,
    Format, KindOption, ValueType,
};
use ferrowl_codec::format::{
    Alignment as TextAlignment, BitField, Endian as RegisterEndian, Format as RegisterFormat,
    Resolution,
};
use ferrowl_codec::{Access, Kind};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{
        ButtonStateBuilder, CodeInputFieldStateBuilder, InputFieldStateBuilder,
        SelectionStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    widgets::{
        ButtonBuilder, CodeInputFieldBuilder, InputFieldBuilder, SelectionBuilder, TextBuilder,
        Widget,
    },
};
use ratatui::layout::{HorizontalAlignment, Margin};

impl EditInputDialog {
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
}
