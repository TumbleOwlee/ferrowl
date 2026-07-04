//! Construction of the free-text register edit dialog widget tree.
//!
//! All widgets are built by the shared, infallible constructors in
//! [`dialog::widgets`](super::super::widgets); user input never reaches this code.

use super::super::widgets;
use super::{EditInputDialog, EditInputDialogBuilder, EditInputDialogFocus, ValueType};
use crate::dialog::NonEmpty;
use ratatui::layout::HorizontalAlignment;

impl EditInputDialog {
    pub fn new() -> Self {
        let mut label = widgets::input::<NonEmpty>("Label", "Custom label...");
        label.state.set_focused(true);

        EditInputDialogBuilder::default()
            .label(label)
            .description(widgets::input_multiline::<String>(
                "Description",
                "Some description...",
            ))
            .slave_id(widgets::input::<u8>("Slave ID", "e.g. 1"))
            .address(widgets::input::<crate::dialog::Address>(
                "Address",
                "100 or 'virtual'",
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
            .boolean_type(widgets::text_boxed(
                ("Type", HorizontalAlignment::Right),
                "Boolean",
                Default::default(),
                false,
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
            .number_resolution(widgets::input_filled::<f64>(
                ("Resolution", HorizontalAlignment::Center),
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
            .value(widgets::input::<String>("Value", "Enter value..."))
            .default_value(widgets::input::<String>(
                "Default",
                "Default value (applied on startup)...",
            ))
            .add_button(widgets::button("ADD PREDEFINED", 1))
            .update_script(widgets::code(
                "Lua Update",
                "-- Lua update script (optional)",
            ))
            .delete_register_button(widgets::button("DELETE", 1))
            .confirm_button(widgets::button("CONFIRM", 1))
            .error(widgets::error_text())
            .success(widgets::success_text())
            .keybinds([
                widgets::keybind("<Space>: press button | <C-f>: fill value | <Tab>: next"),
                widgets::keybind("<Esc>: cancel | <Enter>: confirm / newline"),
            ])
            .focus(EditInputDialogFocus::Label)
            .build()
            .expect("all EditInputDialog fields are set")
    }
}
