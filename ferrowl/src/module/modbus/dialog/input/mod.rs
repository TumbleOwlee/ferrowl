//! Free-text register edit dialog: every register property as an input field.

use super::{
    AccessOption, Alignment, Endian, Format, KindOption, ValueType, WordOrder, parse_address,
};
use crate::config::device::{NamedValue, Scalar};
use crate::dialog::NonEmpty;
use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmEvent};
use derive_builder::Builder;
use ferrowl_codec::format::{
    BitField, Endian as RegisterEndian, Format as RegisterFormat, Resolution, Width,
    WordOrder as RegisterWordOrder,
};
use ferrowl_codec::{Address, Kind, Register, RegisterBuilder, encode};
use ferrowl_modbus::UnitId;
use ferrowl_ui::{
    state::{ButtonState, InputFieldState, SelectionState},
    traits::SetFocus,
    widgets::{Button, GetValue, InputField, Selection, Text, Validate, ValidateResult, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{buffer::Buffer, layout::Rect};
use std::fmt::Debug;

mod build;
mod render;

#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct EditInputDialog {
    #[focus]
    pub label: Widget<InputFieldState, InputField<NonEmpty>>,
    #[focus]
    pub description: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub slave_id: Widget<InputFieldState, InputField<u8>>,
    #[focus]
    pub address: Widget<InputFieldState, InputField<crate::dialog::Address>>,
    #[focus]
    pub kind: Widget<SelectionState<KindOption>, Selection<KindOption>>,
    #[focus]
    pub access: Widget<SelectionState<AccessOption>, Selection<AccessOption>>,
    #[focus(when = { !self.is_boolean_kind() })]
    pub value_type: Widget<SelectionState<ValueType>, Selection<ValueType>>,
    // Static "Boolean" label shown instead of Type selector for Coil/DiscreteInput
    pub boolean_type: Widget<String, Text>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number })]
    pub number_format: Widget<SelectionState<Format>, Selection<Format>>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number })]
    pub number_endian: Widget<SelectionState<Endian>, Selection<Endian>>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number && is_multi_register_format(&self.number_format.get_value().0) })]
    pub number_word_order: Widget<SelectionState<WordOrder>, Selection<WordOrder>>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number })]
    pub number_resolution: Widget<InputFieldState, InputField<f64>>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Number && is_integer_format(&self.number_format.get_value().0) })]
    pub number_bitmask: Widget<InputFieldState, InputField<crate::dialog::Bitmask>>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Text })]
    pub text_alignment: Widget<SelectionState<Alignment>, Selection<Alignment>>,
    #[focus(when = { !self.is_boolean_kind() && self.value_type.get_value() == ValueType::Text })]
    pub text_width: Widget<InputFieldState, InputField<usize>>,
    #[focus(when = {self.access.get_value().0 != ferrowl_codec::Access::ReadOnly || self.is_server })]
    pub value: Widget<InputFieldState, InputField<String>>,
    // Default value stored in the device config and applied on startup
    #[focus(when = {self.access.get_value().0 != ferrowl_codec::Access::ReadOnly || self.is_server })]
    pub default_value: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    #[focus]
    pub confirm_button: Widget<ButtonState, Button>,
    #[focus(when = { self.deletable })]
    pub delete_register_button: Widget<ButtonState, Button>,
    pub error: Widget<String, Text>,
    pub success: Widget<String, Text>,
    pub keybinds: [Widget<String, Text>; 2],
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
    // Named values accumulated via the ADD button in this session.
    #[builder(default)]
    pub pending_named_values: Vec<NamedValue>,
    // Whether this dialog edits an existing register of server (enables the value input).
    #[builder(default)]
    pub is_server: bool,
    // Whether this dialog edits an existing register (enables the delete button).
    #[builder(default)]
    pub deletable: bool,
    #[builder(default)]
    pub confirm_delete: Option<ConfirmDeleteDialog>,
    // Name-conflict error set by the app at confirm time. Survives the per-frame `validate()`
    // refresh (which can't see other registers) until the user edits the dialog again.
    #[builder(default)]
    pub name_error: Option<String>,
    // Confirm-close popup, opened with Esc.
    #[builder(default)]
    pub close_confirm: Option<CloseConfirmDialog>,
    // The register's configured default at dialog open (MB-R-228): carried through unchanged
    // when the Default Value pane is hidden, regardless of any text the pane holds or a later
    // Access toggle — never re-derived from `default_value`'s (possibly unvalidated) raw input.
    #[builder(default)]
    pub seeded_default: Option<Scalar>,
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

    fn value_inputs_visible(&self) -> bool {
        self.is_server || self.access.get_value().0 != ferrowl_codec::Access::ReadOnly
    }

    fn value_input(&self) -> &str {
        if self.value_inputs_visible() {
            self.value.state.input()
        } else {
            ""
        }
    }

    fn default_value_input(&self) -> &str {
        if self.value_inputs_visible() {
            self.default_value.state.input()
        } else {
            ""
        }
    }

    fn resolved_format(&self) -> Result<RegisterFormat, String> {
        Ok(if self.is_boolean_kind() {
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            )
        } else {
            match self.value_type.state.get_value() {
                ValueType::Number => {
                    let selected = self.number_format.state.get_value();
                    let endian = self.number_endian.state.get_value().0;
                    let word_order = self.number_word_order.state.get_value().0;
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
                    with_numeric_parts(&selected.0, endian, word_order, resolution, bitfield)
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
                    RegisterFormat::Ascii(alignment, Width(width))
                }
            }
        })
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
                }
                ValueType::Text => {
                    if let ValidateResult::Error(e) = usize::validate(self.text_width.state.input())
                    {
                        return Err(format!("Width: {e}"));
                    }
                }
            }
        }
        // A virtual register's value goes through `str_to_value` (`Scalar::from_input`) on write,
        // never `encode` (see `set_register_value`), so confirm must not format-check it either.
        if parse_address(self.address.state.input()) != Ok(Address::Virtual) {
            let format = self.resolved_format()?;
            let s = self.value_input();
            if !s.is_empty()
                && let Err(e) = encode(&format, s)
            {
                return Err(format!("Value: cannot convert '{s}' to number [{e}]"));
            }
            let s = self.default_value_input();
            if !s.is_empty()
                && let Err(e) = encode(&format, s)
            {
                return Err(format!(
                    "Default Value: cannot convert '{s}' to number [{e}]"
                ));
            }
        }
        Ok(())
    }
    /// Build the dialog pre-filled from an existing register and its current value. Focus
    /// starts on the value field so editing the value (the common case) works immediately.
    pub fn from_register(
        name: &str,
        description: &str,
        register: &Register,
        value: &str,
        default: Option<&Scalar>,
        is_server: bool,
    ) -> Self {
        let mut dialog = Self::new();
        dialog.deletable = true;
        dialog.is_server = is_server;
        set_input(&mut dialog.label, name);
        set_input(&mut dialog.description, description);
        dialog.seeded_default = default.cloned();
        if let Some(def) = default {
            set_input(&mut dialog.default_value, &def.to_string());
        }
        // Pre-populate the value field so the user can edit or clear it directly.
        let is_ascii = matches!(register.format(), RegisterFormat::Ascii(_, _));
        if is_ascii {
            let value: String = if matches!(
                register.format(),
                RegisterFormat::Ascii(ferrowl_codec::Alignment::Left, _)
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
            RegisterFormat::Ascii(align, width) => {
                dialog.value_type.state.set_selection(1);
                dialog
                    .text_alignment
                    .state
                    .set_selection(alignment_index(align));
                set_input(&mut dialog.text_width, &width.0.to_string());
            }
            numeric => {
                let (endian, word_order, resolution, bitfield) = numeric_parts(numeric);
                dialog.value_type.state.set_selection(0);
                dialog
                    .number_format
                    .state
                    .set_selection(format_index(numeric));
                dialog
                    .number_endian
                    .state
                    .set_selection(endian_index(&endian));
                dialog
                    .number_word_order
                    .state
                    .set_selection(word_order_index(&word_order));
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

        let format = self.resolved_format()?;

        let slave_id = self
            .slave_id
            .state
            .input()
            .trim()
            .parse::<u8>()
            .map_err(|_| "Slave ID must be 0–255.".to_string())?;

        let register = RegisterBuilder::default()
            .slave_id(UnitId(slave_id))
            .access(self.access.state.get_value().0.clone())
            .kind(self.kind.state.get_value().0)
            .address(address)
            .format(format)
            .build()
            .expect("all register fields are set");

        let s = self.value_input();
        let value = if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        };
        let named_values = if self.pending_named_values.is_empty() {
            None
        } else {
            Some(self.pending_named_values.clone())
        };

        // MB-R-228: a pane hidden by MB-R-151 carries the configured default through unchanged
        // rather than unsetting it, so this carries `seeded_default` verbatim — the pane's raw
        // text is never trustworthy here, since Access can toggle it hidden after unchecked text
        // was typed while it was still visible.
        let default = if self.value_inputs_visible() {
            let s = self.default_value_input();
            if s.is_empty() {
                None
            } else {
                Some(Scalar::from_input(s))
            }
        } else {
            self.seeded_default.clone()
        };

        Ok(EditedRegister {
            name,
            description,
            register,
            value,
            named_values,
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
        d.is_server = self.is_server;
        d.label.state = self.label.state.clone();
        d.description.state = self.description.state.clone();
        d.slave_id.state = self.slave_id.state.clone();
        d.address.state = self.address.state.clone();
        d.kind.state = self.kind.state.clone();
        d.access.state = self.access.state.clone();
        d.value_type.state = self.value_type.state.clone();
        d.number_format.state = self.number_format.state.clone();
        d.number_endian.state = self.number_endian.state.clone();
        d.number_word_order.state = self.number_word_order.state.clone();
        d.number_resolution.state = self.number_resolution.state.clone();
        d.number_bitmask.state = self.number_bitmask.state.clone();
        d.text_alignment.state = self.text_alignment.state.clone();
        d.text_width.state = self.text_width.state.clone();

        // Index 0 is the "(no default)" sentinel.
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
    fn is_confirm_button_focused(&self) -> bool {
        self.is_confirm_button_focused()
    }
    fn is_delete_register_button_focused(&self) -> bool {
        self.is_delete_register_button_focused()
    }
    fn apply(&self) -> Result<EditedRegister, String> {
        self.apply()
    }
    fn close_confirm_is_active(&self) -> bool {
        self.close_confirm.is_some()
    }
    fn close_confirm_open(&mut self) {
        self.close_confirm = Some(CloseConfirmDialog::new());
    }
    fn close_confirm_handle_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> CloseConfirmEvent {
        let Some(confirm) = self.close_confirm.as_mut() else {
            return CloseConfirmEvent::Dismiss;
        };
        let event = confirm.handle_key(modifiers, code);
        if !matches!(event, CloseConfirmEvent::Consumed) {
            self.close_confirm = None;
        }
        event
    }
}

use super::{
    AddNamedValueDialog, ConfirmDeleteDialog, SubDialogs, access_index, alignment_index,
    endian_index, format_index, is_integer_format, is_multi_register_format, kind_index,
    numeric_parts, parse_bitmask, set_input, with_numeric_parts, word_order_index,
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
        Resolution, Width, WordOrder as RegisterWordOrder,
    };
    use ferrowl_codec::{Access, Address, Kind, Register, RegisterBuilder};
    use ferrowl_modbus::UnitId;

    fn reg(
        kind: Kind,
        access: Access,
        address: Address,
        slave: u8,
        format: RegisterFormat,
    ) -> Register {
        RegisterBuilder::default()
            .slave_id(UnitId(slave))
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
            RegisterFormat::u32(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let edited =
            EditInputDialog::from_register("temp", "a sensor", &original, "42", None, true)
                .apply()
                .expect("valid register should apply");

        assert_eq!(edited.name, "temp");
        assert_eq!(edited.description, "a sensor");
        assert_eq!(*edited.register.slave_id(), UnitId(7));
        assert_eq!(*edited.register.kind(), Kind::HoldingRegister);
        assert_eq!(*edited.register.access(), Access::ReadWrite);
        assert_eq!(*edited.register.address(), Address::Fixed(100));
        assert_eq!(edited.register.format(), original.format());
        assert_eq!(edited.value.as_deref(), Some("42"));
    }

    #[test]
    /// MB-R-099 — a reversed register order is seeded on open and preserved through apply.
    fn ut_reversed_word_order_round_trips_through_apply() {
        let original = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(10),
            1,
            RegisterFormat::u32(
                RegisterEndian::Big,
                RegisterWordOrder::Reversed,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let edited = EditInputDialog::from_register("w", "", &original, "1", None, true)
            .apply()
            .expect("valid register should apply");
        assert_eq!(edited.register.format(), original.format());
    }

    #[test]
    fn ut_virtual_address_and_read_only_round_trip() {
        let original = reg(
            Kind::InputRegister,
            Access::ReadOnly,
            Address::Virtual,
            1,
            RegisterFormat::u16(
                RegisterEndian::Little,
                RegisterWordOrder::Normal,
                Resolution(0.5),
                BitField::default(),
            ),
        );
        let edited = EditInputDialog::from_register("v", "", &original, "3", None, true)
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
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField { mask: 0xFF00 },
            ),
        );
        let edited = EditInputDialog::from_register("masked", "", &original, "0", None, true)
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
            RegisterFormat::Ascii(TextAlignment::Left, Width(4)),
        );
        let edited = EditInputDialog::from_register("label", "", &original, "AB", None, true)
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
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let edited = EditInputDialog::from_register("c", "", &original, "1", None, true)
            .apply()
            .expect("valid register should apply");
        assert_eq!(*edited.register.kind(), Kind::Coil);
        // Boolean kinds (Coil/DiscreteInput) always serialize as a default big-endian U16.
        assert_eq!(
            *edited.register.format(),
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default()
            )
        );
    }

    #[test]
    fn ut_empty_add_dialog_does_not_apply() {
        // A freshly opened "Add" dialog has an empty Label, so confirming it must fail validation
        // rather than produce a bogus register.
        assert!(EditInputDialog::new().apply().is_err());
    }

    #[test]
    /// MB-R-222 — an empty Value input applies with no value to write, both for a numeric
    /// register and (since MB-R-227 governs hidden inputs only) for an Ascii register too.
    fn ut_empty_value_input_applies_with_no_write() {
        let numeric = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let edited = EditInputDialog::from_register("n", "", &numeric, "", None, true)
            .apply()
            .expect("empty value input should apply");
        assert_eq!(edited.value, None);

        let ascii = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::Ascii(TextAlignment::Left, Width(4)),
        );
        let edited = EditInputDialog::from_register("n", "", &ascii, "", None, true)
            .apply()
            .expect("empty value input should apply");
        assert_eq!(edited.value, None);
    }

    #[test]
    /// MB-R-223 — a non-empty Value input is evaluated and carried through on confirm.
    fn ut_non_empty_value_input_is_evaluated_and_carried() {
        let numeric = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let edited = EditInputDialog::from_register("n", "", &numeric, "42", None, true)
            .apply()
            .expect("valid value input should apply");
        assert_eq!(edited.value, Some("42".to_string()));
    }

    #[test]
    /// MB-R-224 — an invalid Value input refuses confirm with an inline error, repeatably.
    fn ut_invalid_value_input_refuses_confirm() {
        let numeric = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let dialog = EditInputDialog::from_register("n", "", &numeric, "abc", None, true);
        let err = dialog
            .apply()
            .expect_err("invalid value should refuse confirm");
        assert!(err.starts_with("Value: "));
        let err2 = dialog
            .apply()
            .expect_err("second apply should refuse the same way");
        assert!(err2.starts_with("Value: "));
    }

    #[test]
    /// MB-R-225 — an empty Default Value input is never a validation error.
    fn ut_empty_default_value_is_not_an_error() {
        let numeric = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let edited = EditInputDialog::from_register("n", "", &numeric, "1", None, true)
            .apply()
            .expect("empty default value should apply");
        assert_eq!(edited.default, None);
    }

    #[test]
    /// MB-R-226 — an invalid Default Value input refuses confirm with the error on its own input.
    fn ut_invalid_default_value_refuses_confirm_on_its_own_input() {
        let numeric = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let mut dialog = EditInputDialog::from_register("n", "", &numeric, "1", None, true);
        crate::module::modbus::dialog::set_input(&mut dialog.default_value, "abc");
        let err = dialog
            .apply()
            .expect_err("invalid default value should refuse confirm");
        assert!(err.starts_with("Default Value: "));
    }

    #[test]
    /// MB-R-227 — a Value or Default Value pane hidden by MB-R-151 (client, ReadOnly) is never
    /// evaluated and never blocks confirm, regardless of the text it holds.
    fn ut_hidden_value_inputs_count_as_empty() {
        let ro = reg(
            Kind::HoldingRegister,
            Access::ReadOnly,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let mut dialog = EditInputDialog::from_register("n", "", &ro, "", None, false);
        crate::module::modbus::dialog::set_input(&mut dialog.value, "abc");
        crate::module::modbus::dialog::set_input(&mut dialog.default_value, "abc");
        let edited = dialog
            .apply()
            .expect("hidden panes are never evaluated and never block confirm");
        assert_eq!(edited.value, None);
    }

    #[test]
    /// MB-R-228 — confirming with a pane hidden by MB-R-151 writes no value and carries the
    /// register's existing stored value and configured default through unchanged: editing a
    /// client `ReadOnly` register must not unset a previously configured default.
    fn ut_hidden_panes_preserve_existing_value_and_default() {
        let ro = reg(
            Kind::HoldingRegister,
            Access::ReadOnly,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let default = crate::config::device::Scalar::from_input("3");
        let dialog = EditInputDialog::from_register("n", "", &ro, "5", Some(&default), false);
        let edited = dialog.apply().expect("hidden panes never block confirm");
        assert_eq!(edited.value, None);
        assert_eq!(edited.default, Some(default));
    }

    #[test]
    /// MB-R-228 — hiding the Default Value pane by toggling Access to `ReadOnly` after typing an
    /// unchecked value into it must not carry that raw, never-validated text through as the
    /// applied default: a hidden pane carries the register's existing configured default,
    /// unchanged, not whatever text happens to still sit in the widget.
    fn ut_access_toggle_hiding_default_pane_does_not_leak_raw_text() {
        let rw = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let seeded_default = crate::config::device::Scalar::from_input("3");
        // Client dialog (`is_server: false`): the Default Value pane starts visible under
        // `ReadWrite` and typing into it works normally.
        let mut dialog =
            EditInputDialog::from_register("n", "", &rw, "5", Some(&seeded_default), false);
        crate::module::modbus::dialog::set_input(&mut dialog.default_value, "not-a-number");
        // Switch Access to ReadOnly, hiding the pane (MB-R-151) without clearing its raw text.
        dialog
            .access
            .state
            .set_selection(crate::module::modbus::dialog::access_index(
                &Access::ReadOnly,
            ));
        let edited = dialog.apply().expect("hidden panes never block confirm");
        assert_eq!(edited.default, Some(seeded_default));
    }

    #[test]
    /// MB-R-223, MB-R-226, MB-E-094 — a whitespace-only input is non-empty (emptiness is zero
    /// length, never trimmed): an Ascii register takes the all-space value, a numeric register
    /// reports a parse error, on both the Value and Default Value inputs.
    fn ut_whitespace_value_input_is_not_empty() {
        let ascii = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::Ascii(TextAlignment::Left, Width(4)),
        );
        let mut ascii_dialog = EditInputDialog::from_register("n", "", &ascii, "", None, true);
        crate::module::modbus::dialog::set_input(&mut ascii_dialog.value, "  ");
        let edited = ascii_dialog
            .apply()
            .expect("all-space Ascii value should apply");
        assert_eq!(edited.value, Some("  ".to_string()));

        let numeric = reg(
            Kind::HoldingRegister,
            Access::ReadWrite,
            Address::Fixed(0),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let mut numeric_dialog = EditInputDialog::from_register("n", "", &numeric, "1", None, true);
        crate::module::modbus::dialog::set_input(&mut numeric_dialog.value, " ");
        let err = numeric_dialog
            .apply()
            .expect_err("all-space numeric value should refuse confirm");
        assert!(err.starts_with("Value: "));
        assert!(err.contains("' '"));

        let mut numeric_default_dialog =
            EditInputDialog::from_register("n", "", &numeric, "1", None, true);
        crate::module::modbus::dialog::set_input(&mut numeric_default_dialog.default_value, " ");
        let err = numeric_default_dialog
            .apply()
            .expect_err("all-space numeric default value should refuse confirm");
        assert!(err.starts_with("Default Value: "));

        let mut ascii_default_dialog =
            EditInputDialog::from_register("n", "", &ascii, "1", None, true);
        crate::module::modbus::dialog::set_input(&mut ascii_default_dialog.default_value, " ");
        let edited = ascii_default_dialog
            .apply()
            .expect("all-space Ascii default value should apply");
        assert_eq!(
            edited.default,
            Some(crate::config::device::Scalar::from_input(" "))
        );
    }

    #[test]
    /// MB-R-222, MB-R-223, MB-R-224 — a boolean-kind (Coil/DiscreteInput) Value input is
    /// evaluated the same as any other kind: empty applies with no write, valid text applies and
    /// carries, invalid text refuses confirm.
    fn ut_boolean_kind_value_input_is_evaluated() {
        let coil = reg(
            Kind::Coil,
            Access::ReadWrite,
            Address::Fixed(1),
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let dialog = EditInputDialog::from_register("n", "", &coil, "abc", None, true);
        let err = dialog
            .apply()
            .expect_err("invalid boolean value should refuse confirm");
        assert!(err.starts_with("Value: "));

        let dialog = EditInputDialog::from_register("n", "", &coil, "1", None, true);
        let edited = dialog.apply().expect("valid boolean value should apply");
        assert_eq!(edited.value, Some("1".to_string()));

        let dialog = EditInputDialog::from_register("n", "", &coil, "", None, true);
        let edited = dialog.apply().expect("empty boolean value should apply");
        assert_eq!(edited.value, None);
    }

    #[test]
    /// MB-R-223, MB-E-095 — a virtual register's Value/Default Value inputs are evaluated exactly
    /// as a `:set` write is: `:set` on a virtual register goes through `str_to_value`
    /// (`Scalar::from_input`), never `encode`, so confirm must not reject text `encode` would.
    fn ut_virtual_register_value_input_is_not_format_encoded() {
        let virtual_coil = reg(
            Kind::Coil,
            Access::ReadWrite,
            Address::Virtual,
            1,
            RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let dialog = EditInputDialog::from_register("n", "", &virtual_coil, "abc", None, true);
        let edited = dialog
            .apply()
            .expect("virtual register's value is not encode-checked");
        assert_eq!(edited.value, Some("abc".to_string()));
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
        WordOrder as RegisterWordOrder,
    };
    use ferrowl_codec::{Access, Address, Kind, Register, RegisterBuilder};
    use ferrowl_modbus::UnitId;
    use ferrowl_ui::traits::HandleEvents;

    fn numeric_dialog() -> EditInputDialog {
        let register = RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(RegisterFormat::u32(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap();
        // `from_register` focuses the value field and sets the cursor at the end of "4".
        EditInputDialog::from_register("name", "", &register, "4", None, true)
    }

    fn coil_dialog() -> EditInputDialog {
        let register: Register = RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::Coil)
            .address(Address::Fixed(0))
            .format(RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap();
        EditInputDialog::from_register("c", "", &register, "1", None, true)
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
            assert!(
                seen.contains(&expected),
                "cycle missing {expected:?}: {seen:?}"
            );
        }
    }

    #[test]
    /// MB-R-099 — the register-order pane is in the cycle for a multi-register format (U32)
    /// but gated off for a single-register one (U16).
    fn ut_focus_cycle_gates_word_order_on_register_width() {
        let mut multi = numeric_dialog(); // U32
        assert!(
            forward_cycle(&mut multi).contains(&EditInputDialogFocus::NumberWordOrder),
            "multi-register cycle should visit NumberWordOrder"
        );

        let single = RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(RegisterFormat::u16(
                RegisterEndian::Big,
                RegisterWordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap();
        let mut single = EditInputDialog::from_register("name", "", &single, "4", None, true);
        assert!(
            !forward_cycle(&mut single).contains(&EditInputDialogFocus::NumberWordOrder),
            "single-register cycle should skip NumberWordOrder"
        );
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
            assert!(
                !seen.contains(&gated),
                "boolean cycle should skip {gated:?}: {seen:?}"
            );
        }
        // The value field is still reachable.
        assert!(seen.contains(&EditInputDialogFocus::Value));
    }
}
