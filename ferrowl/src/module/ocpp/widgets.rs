//! Shared widget constructors for the OCPP overlays ([`server::detail`](super::server::detail)
//! and [`action_dialog`](super::action_dialog)).
//!
//! Every builder here is invoked with a complete, statically-known configuration (all widget
//! and state builder fields carry defaults), so construction is infallible. The single `expect`
//! per constructor documents that invariant instead of scattering `unwrap()` calls through the
//! overlay build code.

use ferrowl_ui::state::ButtonState;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, TableState, TableStateBuilder},
    style::{ButtonStyle, InputFieldStyle, InputFieldStyleBuilder, TableStyleBuilder},
    widgets::{
        Button, Header, InputField, InputFieldBuilder, Table, TableBuilder, TableEntry, Title,
        Widget,
    },
};
use ratatui::layout::Margin;

/// The border color shared by the styled inputs and tables in these overlays.
pub(super) fn border_style() -> ratatui::style::Style {
    ratatui::style::Style::default()
        .fg(COLOR_SCHEME.border)
        .bg(COLOR_SCHEME.bg)
}

/// The input-field style with [`border_style`] applied to the border.
pub(super) fn bordered_input_style() -> InputFieldStyle {
    InputFieldStyleBuilder::default()
        .border(border_style())
        .build()
        .expect("static input-field style")
}

/// An unfocused, bordered, titled, empty table over `Row`s with an `N`-column `Header`.
pub(super) fn table<Row, Hdr, const N: usize>(
    title: impl Into<Title>,
) -> Widget<TableState<Row, N>, Table<Row, Hdr, N>>
where
    Row: TableEntry<N> + Clone,
    Hdr: Header<N> + Clone,
{
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .expect("static table state"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("static table style"),
            )
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .expect("static table config"),
    }
}

/// A bordered, left-aligned, titled input field with a placeholder and given focus/style.
pub(super) fn input(
    title: impl Into<Title>,
    placeholder: &str,
    focused: bool,
    style: InputFieldStyle,
) -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(focused)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .build()
            .expect("static input-field state"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style)
            .build()
            .expect("static input-field config"),
    }
}

/// An unfocused, center-aligned button labelled `label`.
pub(super) fn button(label: &str) -> Widget<ButtonState, Button> {
    ferrowl_ui::widgets::button(
        label,
        ButtonStyle {
            general: border_style(),
            ..ButtonStyle::default()
        },
        0,
    )
}
