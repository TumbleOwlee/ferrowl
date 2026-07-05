//! Shared widget constructors for the register edit dialogs ([`input`](super::input) and
//! [`selection`](super::selection)).
//!
//! Every builder here is invoked with a complete, statically-known configuration (all widget
//! and state builder fields carry defaults), so construction is infallible. The single
//! `expect` per constructor documents that invariant instead of scattering `unwrap()` calls
//! through the dialog build code. No user input flows into these builders — user input is
//! parsed in the dialogs' `validate()`/`apply()` paths and surfaced as dialog error messages.

use super::{AccessOption, Alignment, Endian, Format, KindOption};
use ferrowl_codec::format::{
    Alignment as TextAlignment, BitField, Endian as RegisterEndian, Format as RegisterFormat,
    Resolution,
};
use ferrowl_codec::{Access, Kind};
use ferrowl_ui::state::ButtonState;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder},
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    traits::ToLabel,
    widgets::{
        Button, InputField, InputFieldBuilder, Selection, SelectionBuilder, Text, TextBuilder,
        Title, Validate, Widget,
    },
};
use ratatui::layout::{HorizontalAlignment, Margin};

/// Standard field margin inside the dialog: no vertical, one column horizontal.
fn field_margin() -> Margin {
    Margin {
        vertical: 0,
        horizontal: 1,
    }
}

/// An unfocused, bordered, titled input field with a placeholder.
pub(super) fn input<T: Validate + Clone>(
    title: impl Into<Title>,
    placeholder: &str,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .build()
            .expect("static input-field state"),
        widget: input_widget(title, false),
    }
}

/// Like [`input`], but rendered multiline (used for the description field).
pub(super) fn input_multiline<T: Validate + Clone>(
    title: impl Into<Title>,
    placeholder: &str,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .build()
            .expect("static input-field state"),
        widget: input_widget(title, true),
    }
}

/// An unfocused input field pre-filled with `content` (cursor at its end).
pub(super) fn input_filled<T: Validate + Clone>(
    title: impl Into<Title>,
    content: &str,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .input(content.to_string())
            .cursor(content.len())
            .disabled(false)
            .build()
            .expect("static input-field state"),
        widget: input_widget(title, false),
    }
}

fn input_widget<T: Validate + Clone>(title: impl Into<Title>, multiline: bool) -> InputField<T> {
    InputFieldBuilder::default()
        .border(Border::Full(Margin::new(1, 0)))
        .title(Some(title.into()))
        .multiline(multiline)
        .margin(field_margin())
        .style(InputFieldStyle::default())
        .build()
        .expect("static input-field config")
}

/// An unfocused, bordered, titled selection over `values`, with entry `selected` picked.
pub(super) fn selection<T: ToLabel + Clone>(
    title: impl Into<Title>,
    values: Vec<T>,
    selected: usize,
) -> Widget<SelectionState<T>, Selection<T>> {
    let mut state = SelectionStateBuilder::default()
        .focused(false)
        .values(values)
        .build()
        .expect("static selection state");
    state.set_selection(selected);
    Widget {
        state,
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .margin(field_margin())
            .style(SelectionStyle::default())
            .build()
            .expect("static selection config"),
    }
}

/// An unfocused, center-aligned button labelled `label`, with `horizontal` outer margin.
pub(super) fn button(label: &str, horizontal: u16) -> Widget<ButtonState, Button> {
    ferrowl_ui::widgets::button(label, ButtonStyle::default(), horizontal)
}

/// A bordered, titled static text box showing `content`.
pub(super) fn text_boxed(
    title: impl Into<Title>,
    content: &str,
    style: TextStyle,
    multiline: bool,
) -> Widget<String, Text> {
    Widget {
        state: content.to_string(),
        widget: TextBuilder::default()
            .title(Some(title.into()))
            .border(Border::Full(Margin::new(1, 0)))
            .margin(field_margin())
            .multiline(multiline)
            .style(style)
            .build()
            .expect("static text config"),
    }
}

/// The (initially empty) error pane, in the scheme's error colors.
pub(super) fn error_text() -> Widget<String, Text> {
    let style = TextStyle {
        general: ratatui::prelude::Style::default()
            .fg(COLOR_SCHEME.error)
            .bg(COLOR_SCHEME.bg),
    };
    text_boxed("Error", "", style, true)
}

/// The success pane, in the scheme's success colors.
pub(super) fn success_text() -> Widget<String, Text> {
    let style = TextStyle {
        general: ratatui::prelude::Style::default()
            .fg(COLOR_SCHEME.success)
            .bg(COLOR_SCHEME.bg),
    };
    text_boxed("Success", "Everything is fine.", style, false)
}

/// One borderless, centered keybind help line.
pub(super) fn keybind(content: &str) -> Widget<String, Text> {
    Widget {
        state: content.to_string(),
        widget: TextBuilder::default()
            .margin(field_margin())
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle::default())
            .build()
            .expect("static text config"),
    }
}

/// The register-kind options, in display order.
pub(super) fn kind_options() -> Vec<KindOption> {
    vec![
        KindOption(Kind::Coil),
        KindOption(Kind::DiscreteInput),
        KindOption(Kind::HoldingRegister),
        KindOption(Kind::InputRegister),
    ]
}

/// The access options, in display order (default selection: ReadWrite = index 2).
pub(super) fn access_options() -> Vec<AccessOption> {
    vec![
        AccessOption(Access::ReadOnly),
        AccessOption(Access::WriteOnly),
        AccessOption(Access::ReadWrite),
    ]
}

/// The numeric format options, in display order.
pub(super) fn format_options() -> Vec<Format> {
    let n = || (RegisterEndian::Big, Resolution(1.0), BitField::default());
    vec![
        Format(RegisterFormat::U8(n())),
        Format(RegisterFormat::U16(n())),
        Format(RegisterFormat::U32(n())),
        Format(RegisterFormat::U64(n())),
        Format(RegisterFormat::U128(n())),
        Format(RegisterFormat::I8(n())),
        Format(RegisterFormat::I16(n())),
        Format(RegisterFormat::I32(n())),
        Format(RegisterFormat::I64(n())),
        Format(RegisterFormat::I128(n())),
        Format(RegisterFormat::F32((RegisterEndian::Big, Resolution(1.0)))),
        Format(RegisterFormat::F64((RegisterEndian::Big, Resolution(1.0)))),
    ]
}

/// The endianness options, in display order.
pub(super) fn endian_options() -> Vec<Endian> {
    vec![Endian(RegisterEndian::Big), Endian(RegisterEndian::Little)]
}

/// The text-alignment options, in display order.
pub(super) fn alignment_options() -> Vec<Alignment> {
    vec![
        Alignment(TextAlignment::Left),
        Alignment(TextAlignment::Right),
    ]
}
