//! Shared field-widget constructors for the modbus and ocpp setup dialogs: a bordered, titled
//! `InputField`/`SuggestInput`/`Selection` built with this project's standard border and margin.
//! `title` takes `impl Into<Title>` so a bare `&str` (left-aligned) or a `(&str,
//! HorizontalAlignment)` tuple both work at the call site, matching `Title`'s own `From` impls.

use ferrowl_ui::{
    Border,
    state::{
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
        SuggestInputState, SuggestInputStateBuilder,
    },
    style::{InputFieldStyle, SelectionStyle},
    traits::{SuggestionProvider, ToLabel},
    widgets::{
        InputField, InputFieldBuilder, Selection, SelectionBuilder, SuggestInput,
        SuggestInputBuilder, Title, Validate, Widget,
    },
};
use ratatui::layout::Margin;

/// Standard field margin inside a dialog: no vertical, one column horizontal.
pub(crate) fn field_margin() -> Margin {
    Margin {
        vertical: 0,
        horizontal: 1,
    }
}

/// A bordered, titled input field with a placeholder, restricted to `T`'s allowed characters.
pub(crate) fn input<T: Validate + Clone>(
    title: impl Into<Title>,
    placeholder: &str,
    style: &InputFieldStyle,
    focused: bool,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(focused)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .allowed_for::<T>()
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .margin(field_margin())
            .style(style.clone())
            .build()
            .expect("all required builder fields are set"),
    }
}

/// Like [`input`], with a `provider`-backed completion popup.
pub(crate) fn suggest_input<T: Validate + Clone, P: SuggestionProvider + Clone>(
    title: impl Into<Title>,
    placeholder: &str,
    style: &InputFieldStyle,
    focused: bool,
    provider: P,
) -> Widget<SuggestInputState<P>, SuggestInput<T, P>> {
    Widget {
        state: SuggestInputStateBuilder::default()
            .field(
                InputFieldStateBuilder::default()
                    .focused(focused)
                    .disabled(false)
                    .placeholder(Some(placeholder.to_string()))
                    .allowed_for::<T>()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .provider(provider)
            .build()
            .expect("all required builder fields are set"),
        widget: SuggestInputBuilder::default()
            .input_field(
                InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(title.into()))
                    .margin(field_margin())
                    .style(style.clone())
                    .build()
                    .expect("all required builder fields are set"),
            )
            .build()
            .expect("all required builder fields are set"),
    }
}

/// A bordered, titled, unfocused selection over `values`.
pub(crate) fn selection<T: ToLabel + Clone>(
    title: impl Into<Title>,
    values: Vec<T>,
    style: &SelectionStyle,
) -> Widget<SelectionState<T>, Selection<T>> {
    Widget {
        state: SelectionStateBuilder::default()
            .focused(false)
            .values(values)
            .build()
            .expect("all required builder fields are set"),
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .margin(field_margin())
            .style(style.clone())
            .build()
            .expect("all required builder fields are set"),
    }
}

/// Overwrites `widget`'s text and places the cursor at its end.
pub(crate) fn set_input<T: Validate + Clone>(
    widget: &mut Widget<InputFieldState, InputField<T>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

/// Overwrites `widget`'s text and places the cursor at its end.
pub(crate) fn set_suggest_input<T: Validate + Clone, P: SuggestionProvider + Clone>(
    widget: &mut Widget<SuggestInputState<P>, SuggestInput<T, P>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}
