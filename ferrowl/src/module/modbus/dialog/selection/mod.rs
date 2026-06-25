//! Selection-based register edit dialog: enum-like properties picked from
//! lists instead of typed.

use super::{
    AccessOption, AddNamedValueDialog, Alignment, ConfirmDeleteDialog, Endian, Format, KindOption,
    SubDialogs, ValueType, access_index, alignment_index, endian_index, format_index,
    is_integer_format, kind_index, numeric_parts, parse_address, parse_bitmask, set_input,
    with_numeric_parts,
};
use crate::config::device::{NamedValue, Scalar};
use crate::dialog::EditedRegister;
use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_codec::format::{BitField, Format as RegisterFormat, Resolution, Width};
use ferrowl_codec::{Address, Register, RegisterBuilder};
use ferrowl_ui::{
    state::{ButtonState, CodeInputFieldState, InputFieldState, SelectionState},
    traits::HandleEvents,
    traits::ToLabel,
    widgets::{
        Button, CodeInputField, GetValue, InputField, Selection, Text, Validate, ValidateResult,
        Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{buffer::Buffer, layout::Rect};
use std::fmt::Debug;

mod build;
mod render;

/// Parse a raw memory string like `[00a0 0001]` into an i64 (big-endian word combination).
pub fn parse_raw_value(raw: &str) -> Option<i64> {
    let inner = raw.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut result: u64 = 0;
    for word in inner.split_whitespace() {
        result = (result << 16) | u64::from_str_radix(word, 16).ok()?;
    }
    Some(result as i64)
}

#[cfg(test)]
mod parse_raw_value_tests {
    use super::parse_raw_value;

    #[test]
    fn ut_parses_single_and_multi_word_big_endian() {
        assert_eq!(parse_raw_value("[0001]"), Some(1));
        assert_eq!(parse_raw_value("[0001 0002]"), Some(0x0001_0002));
        assert_eq!(parse_raw_value("[00a0 0001]"), Some(0x00a0_0001));
        // Empty body folds to zero.
        assert_eq!(parse_raw_value("[]"), Some(0));
        // Surrounding whitespace is tolerated.
        assert_eq!(parse_raw_value("  [00ff]  "), Some(0xff));
    }

    #[test]
    fn ut_rejects_unbracketed_or_non_hex() {
        assert_eq!(parse_raw_value("nope"), None);
        assert_eq!(parse_raw_value("[zz]"), None);
        assert_eq!(parse_raw_value("0001"), None);
    }

    #[test]
    fn ut_inverts_raw_hex_layout() {
        // Mirrors `view::raw_hex`'s `[wwww wwww]` formatting.
        assert_eq!(parse_raw_value("[0000 0000]"), Some(0));
        assert_eq!(parse_raw_value("[ffff ffff]"), Some(0xffff_ffff));
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
    pub label: Widget<InputFieldState, InputField<crate::dialog::NonEmpty>>,
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
    // Bit-field mask input (integer formats only)
    #[focus(when = {self.value_type.get_value() == ValueType::Number && is_integer_format(&self.number_format.get_value().0)})]
    pub number_bitmask: Widget<InputFieldState, InputField<crate::dialog::Bitmask>>,
    // Text alignment selection
    #[focus(when = {self.value_type.get_value() == ValueType::Text})]
    pub text_alignment: Widget<SelectionState<Alignment>, Selection<Alignment>>,
    // Text length input
    #[focus(when = {self.value_type.get_value() == ValueType::Text})]
    pub text_width: Widget<InputFieldState, InputField<usize>>,
    // Value selection
    #[focus(when = {!self.value.state.values().is_empty()})]
    pub value: Widget<SelectionState<V>, Selection<V>>,
    // Add button
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    // Delete button
    #[focus(when = {!self.value.state.values().is_empty()})]
    pub delete_button: Widget<ButtonState, Button>,
    // Default value selection (same options as value, plus a leading "no default" sentinel)
    #[focus(when = {!self.value.state.values().is_empty()})]
    pub default_value: Widget<SelectionState<V>, Selection<V>>,
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
        if let ValidateResult::Error(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        } else if let ValidateResult::Error(e) = u8::validate(self.slave_id.state.input()) {
            return Err(format!("Slave ID: {e}"));
        } else if let Err(e) = parse_address(self.address.state.input()) {
            return Err(format!("Address: {e}"));
        }

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
            }
            ValueType::Text => {
                if let ValidateResult::Error(e) = usize::validate(self.text_width.state.input()) {
                    return Err(format!("Width: {e}"));
                }
            }
        }
        Ok(())
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
        d.number_bitmask.state = self.number_bitmask.state.clone();
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

impl SubDialogs for EditSelectionDialog<NamedValue> {
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
        self.value.state.values_mut().push(nv.clone());
        let idx = self.value.state.values().len() - 1;
        self.value.state.set_selection(idx);
        // Keep default selection in sync: append after the sentinel.
        self.default_value.state.values_mut().push(nv);
    }
}

impl super::RegisterDialog for EditSelectionDialog<NamedValue> {
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
