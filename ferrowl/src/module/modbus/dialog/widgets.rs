//! Shared widget constructors for the register edit dialogs ([`input`](super::input) and
//! [`selection`](super::selection)).
//!
//! Every builder here is invoked with a complete, statically-known configuration (all widget
//! and state builder fields carry defaults), so construction is infallible. The single
//! `expect` per constructor documents that invariant instead of scattering `unwrap()` calls
//! through the dialog build code. No user input flows into these builders — user input is
//! parsed in the dialogs' `validate()`/`apply()` paths and surfaced as dialog error messages.

use super::{AccessOption, Alignment, Endian, Format, KindOption, WordOrder};
use crate::dialog::widgets::field_margin;
use ferrowl_codec::format::{
    Alignment as TextAlignment, BitField, Endian as RegisterEndian, FloatFormat, FloatKind,
    Format as RegisterFormat, IntKind, NumericFormat, Resolution, WordOrder as RegisterWordOrder,
};
use ferrowl_codec::{Access, Kind};
use ferrowl_ui::state::ButtonState;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState},
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    traits::ToLabel,
    widgets::{
        Button, InputField, InputFieldBuilder, Selection, Text, TextBuilder, Title, Validate,
        Widget,
    },
};
use ratatui::layout::{HorizontalAlignment, Margin};

/// An unfocused, bordered, titled input field with a placeholder.
pub(crate) fn input<T: Validate + Clone>(
    title: impl Into<Title>,
    placeholder: &str,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .allowed_for::<T>()
            .build()
            .expect("static input-field state"),
        widget: input_widget(title, false),
    }
}

/// Like [`input`], but rendered multiline (used for the description field).
pub(crate) fn input_multiline<T: Validate + Clone>(
    title: impl Into<Title>,
    placeholder: &str,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .allowed_for::<T>()
            .build()
            .expect("static input-field state"),
        widget: input_widget(title, true),
    }
}

/// An unfocused input field pre-filled with `content` (cursor at its end).
pub(crate) fn input_filled<T: Validate + Clone>(
    title: impl Into<Title>,
    content: &str,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .input(content.to_string())
            .cursor(content.len())
            .disabled(false)
            .allowed_for::<T>()
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
pub(crate) fn selection<T: ToLabel + Clone>(
    title: impl Into<Title>,
    values: Vec<T>,
    selected: usize,
) -> Widget<SelectionState<T>, Selection<T>> {
    let mut widget = crate::dialog::widgets::selection(title, values, &SelectionStyle::default());
    widget.state.set_selection(selected);
    widget
}

/// An unfocused, center-aligned button labelled `label`, with `horizontal` outer margin.
pub(crate) fn button(label: &str, horizontal: u16) -> Widget<ButtonState, Button> {
    ferrowl_ui::widgets::button(label, ButtonStyle::default(), horizontal)
}

/// A bordered, titled static text box showing `content`.
pub(crate) fn text_boxed(
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
pub(crate) fn error_text() -> Widget<String, Text> {
    let style = TextStyle {
        general: ratatui::prelude::Style::default()
            .fg(COLOR_SCHEME.error)
            .bg(COLOR_SCHEME.bg),
    };
    text_boxed("Error", "", style, true)
}

/// The success pane, in the scheme's success colors.
pub(crate) fn success_text() -> Widget<String, Text> {
    let style = TextStyle {
        general: ratatui::prelude::Style::default()
            .fg(COLOR_SCHEME.success)
            .bg(COLOR_SCHEME.bg),
    };
    text_boxed("Success", "Everything is fine.", style, false)
}

/// One borderless, centered keybind help line.
pub(crate) fn keybind(content: &str) -> Widget<String, Text> {
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
pub(crate) fn kind_options() -> Vec<KindOption> {
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
pub(crate) fn format_options() -> Vec<Format> {
    IntKind::ALL
        .into_iter()
        .map(|kind| {
            Format(RegisterFormat::Numeric(NumericFormat {
                kind,
                endian: RegisterEndian::Big,
                word_order: RegisterWordOrder::Normal,
                resolution: Resolution(1.0),
                bit_field: BitField::default(),
            }))
        })
        .chain(FloatKind::ALL.into_iter().map(|kind| {
            Format(RegisterFormat::Float(FloatFormat {
                kind,
                endian: RegisterEndian::Big,
                word_order: RegisterWordOrder::Normal,
                resolution: Resolution(1.0),
            }))
        }))
        .collect()
}

/// The endianness options, in display order.
pub(crate) fn endian_options() -> Vec<Endian> {
    vec![Endian(RegisterEndian::Big), Endian(RegisterEndian::Little)]
}

/// The register (word) order options, in display order.
pub(crate) fn word_order_options() -> Vec<WordOrder> {
    vec![
        WordOrder(RegisterWordOrder::Normal),
        WordOrder(RegisterWordOrder::Reversed),
    ]
}

/// The text-alignment options, in display order.
pub(crate) fn alignment_options() -> Vec<Alignment> {
    vec![
        Alignment(TextAlignment::Left),
        Alignment(TextAlignment::Right),
    ]
}
