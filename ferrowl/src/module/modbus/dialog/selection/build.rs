//! Construction of the selection-based register edit dialog widget tree.
//!
//! All widgets are built by the shared, infallible constructors in
//! [`dialog::widgets`](super::super::widgets); user input never reaches this code.

use super::super::widgets;
use super::{EditSelectionDialog, EditSelectionDialogBuilder, EditSelectionDialogFocus, ValueType};
use ferrowl_ui::traits::{SetFocus, ToLabel};
use ratatui::layout::HorizontalAlignment;

impl<V: ToLabel + Clone> EditSelectionDialog<V> {
    pub fn new(values: Vec<V>) -> Self {
        let default_values = values.clone();
        let mut value = widgets::selection("Value", values, 0);
        value.state.set_focused(true);

        EditSelectionDialogBuilder::<V>::default()
            .label(widgets::input::<crate::dialog::NonEmpty>(
                "Label",
                "Custom label...",
            ))
            .description(widgets::input_multiline::<String>(
                "Description",
                "Some description...",
            ))
            .slave_id(widgets::input::<u8>("Slave ID", "e.g. 1"))
            .address(widgets::input::<crate::dialog::Address>(
                "Address", "e.g. 100",
            ))
            .kind(widgets::selection("Kind", widgets::kind_options(), 0))
            .access(widgets::selection(
                "Access",
                widgets::access_options(),
                2, // ReadWrite
            ))
            .value_type(widgets::selection(
                ("Type", HorizontalAlignment::Right),
                vec![ValueType::Number, ValueType::Text],
                0,
            ))
            .number_format(widgets::selection(
                ("Format", HorizontalAlignment::Left),
                widgets::format_options(),
                0,
            ))
            .number_endian(widgets::selection(
                ("Endian", HorizontalAlignment::Center),
                widgets::endian_options(),
                0,
            ))
            .number_word_order(widgets::selection(
                ("Order", HorizontalAlignment::Center),
                widgets::word_order_options(),
                0,
            ))
            .number_resolution(widgets::input_filled::<f64>(
                ("Resolution", HorizontalAlignment::Right),
                "1.0",
            ))
            .number_bitmask(widgets::input::<crate::dialog::Bitmask>(
                ("Bitmask", HorizontalAlignment::Right),
                "0xFFFF",
            ))
            .text_alignment(widgets::selection(
                "Alignment",
                widgets::alignment_options(),
                0,
            ))
            .text_width(widgets::input::<usize>(
                ("Width", HorizontalAlignment::Right),
                "1",
            ))
            .value(value)
            .default_value(widgets::selection("Default", default_values, 0))
            .add_button(widgets::button("ADD", 0))
            .delete_button(widgets::button("DEL", 0))
            .delete_register_button(widgets::button("DELETE", 1))
            .confirm_button(widgets::button("Confirm", 1))
            .error(widgets::error_text())
            .success(widgets::success_text())
            .keybinds([
                widgets::keybind("<Tab>: next | <Space>: press button"),
                widgets::keybind("<Esc>: close | <Enter>: confirm / newline"),
            ])
            .focus(EditSelectionDialogFocus::Value)
            .build()
            .expect("all EditSelectionDialog fields are set")
    }
}
