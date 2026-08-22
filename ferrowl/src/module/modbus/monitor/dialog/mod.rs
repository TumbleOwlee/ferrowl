//! [`EditInterpretationDialog`] (UI-R-061/UI-R-064): a purpose-built `:add`/`:a`/edit dialog for
//! a monitor module, producing a [`MonitorRegisterDef`] (MB-R-145) scoped to the view's
//! currently selected unit id (the dialog itself never asks for a slave id). One struct serves
//! both `:add` (`deletable == false`, `new()`) and the MB-R-148 edit-on-a-row dialog
//! (`deletable == true`, `from_interpretation(...)`) — mirroring the modbus module's own
//! `EditInputDialog`'s `deletable` field/`from_register` split (`dialog/input/mod.rs`), rather
//! than the two near-duplicate structs this module used to keep.
//!
//! Deliberately a new, small struct rather than a "monitor mode" bolted onto
//! [`crate::module::modbus::dialog::EditInputDialog`]: that struct's `access`/`value`/
//! `default_value` fields have no monitor equivalent (a monitor never owns a store cell to
//! write), and it is already a large `#[focus(when = ...)]`-conditional struct — tangling two
//! genuinely different field sets there would make it harder to follow for no shared benefit.

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_codec::Kind;
use ferrowl_codec::format::{
    BitField, Endian as RegisterEndian, Format as RegisterFormat, WordOrder as RegisterWordOrder,
};
use ferrowl_ui::COLOR_SCHEME;
use ferrowl_ui::{
    state::{ButtonState, InputFieldState, SelectionState},
    traits::{HandleEvents, SetFocus},
    widgets::{Button, GetValue, InputField, Selection, Text, Validate, ValidateResult, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    style::Style,
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

use crate::config::device::{
    AlignmentCfg, EndianCfg, MonitorRegisterDef, NamedValue, WordOrderCfg,
};
use crate::dialog::NonEmpty;
use crate::module::modbus::dialog::{
    AddNamedValueDialog, Alignment, ConfirmDeleteDialog, Endian, Format, KindOption, ValueType,
    WordOrder, alignment_index, endian_index, format_index, is_integer_format,
    is_multi_register_format, kind_index, numeric_parts, parse_address, parse_bitmask, set_input,
    widgets, word_order_index,
};

/// UI-R-061 (`:add`, `deletable == false`) / MB-R-148 (edit-on-a-Resolved-registers-row,
/// `deletable == true`) dialog: the field set plus a Delete button/confirmation flow gated on
/// `deletable`, and prefill-from-existing-row support (`from_interpretation`). Mirrors the
/// modbus module's own `EditInputDialog`'s `deletable` field / `new()` vs `from_register` split
/// (`dialog/input/mod.rs`, Shared) — a single struct rather than the two near-duplicate structs
/// (`AddInterpretationDialog`/`EditInterpretationDialog`) this module used to keep, which
/// differed only in that gated Delete button and the mode-switch-to-selection wiring. A new,
/// small struct rather than a mode bolted onto `EditInputDialog` itself, for the same reason
/// this module's doc comment gives (Shared): no `access`/`value`/`default_value` fields apply to
/// a monitor interpretation.
#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct EditInterpretationDialog {
    #[focus]
    pub label: Widget<InputFieldState, InputField<NonEmpty>>,
    #[focus]
    pub description: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub address: Widget<InputFieldState, InputField<crate::dialog::Address>>,
    #[focus]
    pub kind: Widget<SelectionState<KindOption>, Selection<KindOption>>,
    #[focus(when = { !self.is_boolean_kind() })]
    pub value_type: Widget<SelectionState<ValueType>, Selection<ValueType>>,
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
    /// Manual-exercise fix (item 3) — while `pending_named_values` is empty, this is a
    /// full-width "ADD ALIAS" button; the moment the first alias is added, the dialog converts
    /// into an [`EditInterpretationSelectionDialog`] (`to_selection_dialog`), which owns the
    /// alias list + DEL UI instead — mirrors the modbus module's own `EditInputDialog`/
    /// `EditSelectionDialog` mode swap (Shared).
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    #[focus]
    pub confirm_button: Widget<ButtonState, Button>,
    /// Deletes the interpretation outright (MB-R-148), guarded by `confirm_delete` — only
    /// focusable when `deletable` (mirrors `EditInputDialog`'s `delete_register_button`, Shared).
    #[focus(when = { self.deletable })]
    pub delete_button: Widget<ButtonState, Button>,
    pub error: Widget<String, Text>,
    pub keybinds: [Widget<String, Text>; 2],
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
    #[builder(default)]
    pub pending_named_values: Vec<NamedValue>,
    /// Whether this dialog edits an existing interpretation (enables the delete button) —
    /// `false` for `:add`/`new()`, `true` for `from_interpretation` (MB-R-148). Mirrors
    /// `EditInputDialog::deletable` (Shared).
    #[builder(default)]
    pub deletable: bool,
    /// Guards `delete_button` (MB-R-148) — reuses `ConfirmDeleteDialog` verbatim, already
    /// generic sub-dialog plumbing (Shared), not `EditInputDialog`-specific.
    #[builder(default)]
    pub confirm_delete: Option<ConfirmDeleteDialog>,
}

impl EditInterpretationDialog {
    pub fn new() -> Self {
        let mut label = widgets::input::<NonEmpty>("Label", "Custom label...");
        ferrowl_ui::traits::SetFocus::set_focused(&mut label.state, true);

        EditInterpretationDialogBuilder::default()
            .label(label)
            .description(widgets::input_multiline::<String>(
                "Description",
                "Some description...",
            ))
            .address(widgets::input::<crate::dialog::Address>(
                "Address",
                "100 or 'virtual'",
            ))
            .kind(widgets::selection("Kind", widgets::kind_options(), 0))
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
            .number_word_order(widgets::selection(
                ("Order", HorizontalAlignment::Center),
                widgets::word_order_options(),
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
            .add_button(widgets::button("ADD ALIAS", 1))
            .confirm_button(widgets::button("CONFIRM", 1))
            .delete_button(widgets::button("DELETE", 1))
            .error(widgets::error_text())
            .keybinds([
                widgets::keybind("<Space>: press button | <C-f>: fill value | <Tab>: next"),
                widgets::keybind("<Esc>: close | <Enter>: confirm / newline"),
            ])
            .focus(EditInterpretationDialogFocus::Label)
            .build()
            .expect("all EditInterpretationDialog fields are set")
    }

    /// Build the dialog pre-filled from `name`/`def` (MB-R-148). Focus starts on Address (the
    /// common edit target), same reasoning `EditInputDialog::from_register`'s own doc comment
    /// gives for starting on Value there.
    pub fn from_interpretation(name: &str, def: &MonitorRegisterDef) -> Self {
        let mut dialog = Self::new();
        dialog.deletable = true;
        set_input(&mut dialog.label, name);
        set_input(&mut dialog.description, &def.description);
        match def.address() {
            ferrowl_codec::Address::Fixed(a) => set_input(&mut dialog.address, &a.to_string()),
            ferrowl_codec::Address::Virtual => set_input(&mut dialog.address, "virtual"),
        }
        dialog.kind.state.set_selection(kind_index(&def.kind));

        match def.format() {
            RegisterFormat::Ascii((align, width)) => {
                dialog.value_type.state.set_selection(1);
                dialog
                    .text_alignment
                    .state
                    .set_selection(alignment_index(&align));
                set_input(&mut dialog.text_width, &width.0.to_string());
            }
            numeric => {
                let (endian, word_order, resolution, bitfield) = numeric_parts(&numeric);
                dialog.value_type.state.set_selection(0);
                dialog
                    .number_format
                    .state
                    .set_selection(format_index(&numeric));
                dialog
                    .number_endian
                    .state
                    .set_selection(endian_index(&endian));
                dialog
                    .number_word_order
                    .state
                    .set_selection(word_order_index(&word_order));
                set_input(&mut dialog.number_resolution, &resolution.0.to_string());
                if !bitfield.is_full() {
                    set_input(
                        &mut dialog.number_bitmask,
                        &format!("0x{:X}", bitfield.mask),
                    );
                }
            }
        }
        dialog.pending_named_values = def.values.clone();
        dialog.label.state.set_focused(false);
        dialog.address.state.set_focused(true);
        dialog.focus = EditInterpretationDialogFocus::Address;
        dialog
    }

    fn is_boolean_kind(&self) -> bool {
        matches!(
            self.kind.state.get_value().0,
            Kind::Coil | Kind::DiscreteInput
        )
    }

    fn validate(&self) -> Result<(), String> {
        if let ValidateResult::Error(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
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
        Ok(())
    }

    /// Validate and produce `(name, MonitorRegisterDef)`. `slave_id` is left at its default
    /// (`0`) — the caller (the view) scopes it to the currently selected unit id (UI-R-061 for
    /// `:add`; UI-R-064's edit dialog scopes to the unit id already being edited).
    pub fn apply(&self) -> Result<(String, MonitorRegisterDef), String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let description = self.description.state.input().trim().to_string();
        let address = parse_address(self.address.state.input())?;
        let address = match address {
            ferrowl_codec::Address::Fixed(a) => Some(a),
            ferrowl_codec::Address::Virtual => None,
        };
        let is_virtual = address.is_none();
        let kind = self.kind.state.get_value().0.clone();

        let (value_type, endian, word_order, resolution, bitmask, alignment, length) =
            if self.is_boolean_kind() {
                (
                    crate::config::device::ValueType::U16,
                    EndianCfg::Big,
                    WordOrderCfg::Normal,
                    1.0,
                    None,
                    AlignmentCfg::Left,
                    1usize,
                )
            } else {
                match self.value_type.state.get_value() {
                    ValueType::Number => {
                        let selected = self.number_format.state.get_value();
                        let endian = self.number_endian.state.get_value().0.clone();
                        let word_order = self.number_word_order.state.get_value().0;
                        let resolution = self
                            .number_resolution
                            .state
                            .input()
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| "Resolution must be a number.".to_string())?;
                        let bitfield = if is_integer_format(&selected.0) {
                            parse_bitmask(self.number_bitmask.state.input())
                                .map_err(|e| format!("Bitmask {e}."))?
                        } else {
                            BitField::default()
                        };
                        let bitmask = if bitfield.is_full() {
                            None
                        } else {
                            Some(format!("0x{:X}", bitfield.mask))
                        };
                        (
                            value_type_from_format(&selected.0),
                            endian_cfg(&endian),
                            word_order_cfg(word_order),
                            resolution,
                            bitmask,
                            AlignmentCfg::Left,
                            1,
                        )
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
                        (
                            crate::config::device::ValueType::Ascii,
                            EndianCfg::Big,
                            WordOrderCfg::Normal,
                            1.0,
                            None,
                            alignment_cfg(alignment),
                            width,
                        )
                    }
                }
            };

        let def = MonitorRegisterDef {
            slave_id: 0,
            kind,
            address,
            is_virtual,
            value_type,
            endian,
            word_order,
            resolution,
            bitmask,
            length,
            alignment,
            values: self.pending_named_values.clone(),
            description,
            default: None,
        };
        Ok((name, def))
    }

    pub fn open_add_dialog(&mut self) {
        self.add_dialog = Some(AddNamedValueDialog::new());
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

    /// Open the delete-confirmation popup (MB-R-148), named after the dialog's current label
    /// input (mirrors `SubDialogs::open_confirm_delete`'s `register_label`, Shared).
    pub fn open_confirm_delete(&mut self) {
        let name = self.label.state.input().to_string();
        self.confirm_delete = Some(ConfirmDeleteDialog::new(&name));
    }

    /// Manual-exercise fix (item 3) — carry every shared field's state over into a fresh
    /// [`EditInterpretationSelectionDialog`], seeded with `pending_named_values` as its alias
    /// list. Called once the first alias is added, mirroring the modbus module's own
    /// `EditInputDialog::to_edit_selection_dialog` (Shared).
    pub fn to_selection_dialog(&self) -> EditInterpretationSelectionDialog {
        let mut d = EditInterpretationSelectionDialog::new(self.pending_named_values.clone());
        d.deletable = self.deletable;
        d.label.state = self.label.state.clone();
        d.description.state = self.description.state.clone();
        d.address.state = self.address.state.clone();
        d.kind.state = self.kind.state.clone();
        d.value_type.state = self.value_type.state.clone();
        d.number_format.state = self.number_format.state.clone();
        d.number_endian.state = self.number_endian.state.clone();
        d.number_word_order.state = self.number_word_order.state.clone();
        d.number_resolution.state = self.number_resolution.state.clone();
        d.number_bitmask.state = self.number_bitmask.state.clone();
        d.text_alignment.state = self.text_alignment.state.clone();
        d.text_width.state = self.text_width.state.clone();
        // Cloning each field's `.state` above also clones its own `focused` bool (e.g.
        // `from_interpretation` starts focus on Address); re-derive widget-level focus from
        // `d.focus` (Selection mode's own default, `Value`) so exactly one field ends up
        // focused, not two.
        SetFocus::set_focused(&mut d, true);
        d
    }

    pub fn handle_space(&mut self) {
        match self.focus {
            EditInterpretationDialogFocus::AddButton => self.open_add_dialog(),
            EditInterpretationDialogFocus::DeleteButton => self.open_confirm_delete(),
            _ => {
                let _ = HandleEvents::handle_events(self, KeyModifiers::NONE, KeyCode::Char(' '));
            }
        }
    }

    pub fn is_confirm_button_focused(&self) -> bool {
        matches!(self.focus, EditInterpretationDialogFocus::ConfirmButton)
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.validate() {
            Ok(()) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(76), Constraint::Min(1)])
                .areas(area);
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(30),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg))
            .title_alignment(HorizontalAlignment::Center)
            .title(if self.deletable {
                "Edit interpretation"
            } else {
                "Add interpretation"
            });
        let dialog_box = vertical_layout[1];
        let block_inner = block.inner(dialog_box);
        let area = block_inner.inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, dialog_box, buf);
        block.render(dialog_box, buf);

        let rows: [Rect; 10] = Layout::vertical([
            Constraint::Length(3), // 0 label
            Constraint::Length(3), // 1 description
            Constraint::Length(3), // 2 address
            Constraint::Length(3), // 3 kind + type (value_type / boolean_type)
            Constraint::Length(3), // 4 Number/Text-conditional fields
            Constraint::Length(3), // 5 add_button
            Constraint::Length(3), // 6 confirm_button [+ delete_button when deletable]
            Constraint::Length(3), // 7 error
            Constraint::Length(1), // 8 keybind0
            Constraint::Length(1), // 9 keybind1
        ])
        .areas(area);

        StatefulWidget::render(&self.label.widget, rows[0], buf, &mut self.label.state);
        StatefulWidget::render(
            &self.description.widget,
            rows[1],
            buf,
            &mut self.description.state,
        );
        StatefulWidget::render(&self.address.widget, rows[2], buf, &mut self.address.state);
        // Kind and Type on the same line (Shared).
        let kind_type: [Rect; 2] =
            Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(rows[3]);
        StatefulWidget::render(&self.kind.widget, kind_type[0], buf, &mut self.kind.state);
        if self.is_boolean_kind() {
            StatefulWidget::render(
                &self.boolean_type.widget,
                kind_type[1],
                buf,
                &mut self.boolean_type.state,
            );
        } else {
            StatefulWidget::render(
                &self.value_type.widget,
                kind_type[1],
                buf,
                &mut self.value_type.state,
            );
            match self.value_type.state.values()[self.value_type.state.selection()] {
                ValueType::Number => {
                    let integer = is_integer_format(&self.number_format.get_value().0);
                    let multi = is_multi_register_format(&self.number_format.get_value().0);
                    let columns = 3 + multi as usize + integer as usize;
                    let cells =
                        Layout::horizontal(vec![Constraint::Min(1); columns]).split(rows[4]);

                    let mut col = 0;
                    StatefulWidget::render(
                        &self.number_format.widget,
                        cells[col],
                        buf,
                        &mut self.number_format.state,
                    );
                    col += 1;

                    StatefulWidget::render(
                        &self.number_endian.widget,
                        cells[col],
                        buf,
                        &mut self.number_endian.state,
                    );
                    col += 1;

                    if multi {
                        StatefulWidget::render(
                            &self.number_word_order.widget,
                            cells[col],
                            buf,
                            &mut self.number_word_order.state,
                        );
                        col += 1;
                    }

                    StatefulWidget::render(
                        &self.number_resolution.widget,
                        cells[col],
                        buf,
                        &mut self.number_resolution.state,
                    );
                    col += 1;

                    if integer {
                        StatefulWidget::render(
                            &self.number_bitmask.widget,
                            cells[col],
                            buf,
                            &mut self.number_bitmask.state,
                        );
                    }
                }
                ValueType::Text => {
                    let cells: [Rect; 2] =
                        Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(rows[4]);
                    StatefulWidget::render(
                        &self.text_alignment.widget,
                        cells[0],
                        buf,
                        &mut self.text_alignment.state,
                    );
                    StatefulWidget::render(
                        &self.text_width.widget,
                        cells[1],
                        buf,
                        &mut self.text_width.state,
                    );
                }
            }
        }
        // A plain, full-width "ADD ALIAS" button; the alias list/DEL UI lives entirely in
        // `EditInterpretationSelectionDialog` (Shared).
        StatefulWidget::render(
            &self.add_button.widget,
            rows[5],
            buf,
            &mut self.add_button.state,
        );
        // Confirm alone full-width when `:add` (`!deletable`); Confirm+Delete 50/50 split once
        // editing an existing interpretation (`deletable`) — matches `EditInputDialog::render`'s
        // own `deletable` branch (Shared).
        if self.deletable {
            let confirm_delete: [Rect; 2] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(rows[6]);
            StatefulWidget::render(
                &self.confirm_button.widget,
                confirm_delete[0],
                buf,
                &mut self.confirm_button.state,
            );
            StatefulWidget::render(
                &self.delete_button.widget,
                confirm_delete[1],
                buf,
                &mut self.delete_button.state,
            );
        } else {
            StatefulWidget::render(
                &self.confirm_button.widget,
                rows[6],
                buf,
                &mut self.confirm_button.state,
            );
        }
        // Same class of bug as `setup_dialog::MonitorSetupDialog::render` (Shared): no error box
        // drawn at all while the dialog validates cleanly.
        if !self.error.state.is_empty() {
            StatefulWidget::render(&self.error.widget, rows[7], buf, &mut self.error.state);
        }
        StatefulWidget::render(
            &self.keybinds[0].widget,
            rows[8],
            buf,
            &mut self.keybinds[0].state,
        );
        StatefulWidget::render(
            &self.keybinds[1].widget,
            rows[9],
            buf,
            &mut self.keybinds[1].state,
        );

        if let Some(dialog) = self.add_dialog.as_mut() {
            dialog.render(area, buf);
        }
        if let Some(confirm) = self.confirm_delete.as_mut() {
            confirm.render(area, buf);
        }
    }
}

/// Manual-exercise fix (item 3) — the "mode-switch" counterpart to [`EditInterpretationDialog`]:
/// takes over once the first alias is added, owning the alias list + DEL UI (mirrors the modbus
/// module's own `EditInputDialog`/`EditSelectionDialog` split, `dialog/input/mod.rs` and
/// `dialog/selection/mod.rs` — Shared). Not `EditSelectionDialog<NamedValue>` reused directly:
/// that type's `access`/`slave_id`/`default_value` fields have no monitor equivalent (UI-R-064
/// drops Access; slave id is scoped externally by the selected unit id, never asked in either
/// monitor dialog; a monitor interpretation has no "default" concept — it never writes to a
/// store cell).
#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct EditInterpretationSelectionDialog {
    #[focus]
    pub label: Widget<InputFieldState, InputField<NonEmpty>>,
    #[focus]
    pub description: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub address: Widget<InputFieldState, InputField<crate::dialog::Address>>,
    #[focus]
    pub kind: Widget<SelectionState<KindOption>, Selection<KindOption>>,
    #[focus(when = { !self.is_boolean_kind() })]
    pub value_type: Widget<SelectionState<ValueType>, Selection<ValueType>>,
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
    /// The alias list itself — the source of truth `apply()` reads (unlike
    /// `EditInterpretationDialog::pending_named_values`, its `Input`-mode counterpart's own
    /// separate list).
    #[focus(when = { !self.value.state.values().is_empty() })]
    pub value: Widget<SelectionState<NamedValue>, Selection<NamedValue>>,
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    #[focus(when = { !self.value.state.values().is_empty() })]
    pub delete_value_button: Widget<ButtonState, Button>,
    #[focus]
    pub confirm_button: Widget<ButtonState, Button>,
    /// Deletes the interpretation outright (MB-R-148), guarded by `confirm_delete` — only
    /// focusable when `deletable`, same as `EditInterpretationDialog::delete_button` (Shared).
    #[focus(when = { self.deletable })]
    pub delete_button: Widget<ButtonState, Button>,
    pub error: Widget<String, Text>,
    pub keybinds: [Widget<String, Text>; 2],
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
    /// Whether this dialog edits an existing interpretation (enables the delete button); carried
    /// over from `EditInterpretationDialog::deletable` across the mode swap in both directions
    /// (`to_selection_dialog`/`to_input_dialog`, Shared).
    #[builder(default)]
    pub deletable: bool,
    #[builder(default)]
    pub confirm_delete: Option<ConfirmDeleteDialog>,
}

impl EditInterpretationSelectionDialog {
    pub fn new(values: Vec<NamedValue>) -> Self {
        let mut label = widgets::input::<NonEmpty>("Label", "Custom label...");
        ferrowl_ui::traits::SetFocus::set_focused(&mut label.state, false);
        let mut value = widgets::selection("Value", values, 0);
        value.state.set_focused(true);

        EditInterpretationSelectionDialogBuilder::default()
            .label(label)
            .description(widgets::input_multiline::<String>(
                "Description",
                "Some description...",
            ))
            .address(widgets::input::<crate::dialog::Address>(
                "Address",
                "100 or 'virtual'",
            ))
            .kind(widgets::selection("Kind", widgets::kind_options(), 0))
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
            .number_word_order(widgets::selection(
                ("Order", HorizontalAlignment::Center),
                widgets::word_order_options(),
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
            .value(value)
            .add_button(widgets::button("ADD ALIAS", 1))
            .delete_value_button(widgets::button("DEL", 0))
            .confirm_button(widgets::button("CONFIRM", 1))
            .delete_button(widgets::button("DELETE", 1))
            .error(widgets::error_text())
            .keybinds([
                widgets::keybind("<Space>: press button | <C-f>: fill value | <Tab>: next"),
                widgets::keybind("<Esc>: close | <Enter>: confirm / newline"),
            ])
            .focus(EditInterpretationSelectionDialogFocus::Value)
            .build()
            .expect("all EditInterpretationSelectionDialog fields are set")
    }

    /// Counterpart to `EditInterpretationDialog::to_selection_dialog` — carry every shared
    /// field's state back once the alias list empties out (Shared).
    pub fn to_input_dialog(&self) -> EditInterpretationDialog {
        let mut d = EditInterpretationDialog::new();
        d.deletable = self.deletable;
        d.label.state = self.label.state.clone();
        d.description.state = self.description.state.clone();
        d.address.state = self.address.state.clone();
        d.kind.state = self.kind.state.clone();
        d.value_type.state = self.value_type.state.clone();
        d.number_format.state = self.number_format.state.clone();
        d.number_endian.state = self.number_endian.state.clone();
        d.number_word_order.state = self.number_word_order.state.clone();
        d.number_resolution.state = self.number_resolution.state.clone();
        d.number_bitmask.state = self.number_bitmask.state.clone();
        d.text_alignment.state = self.text_alignment.state.clone();
        d.text_width.state = self.text_width.state.clone();
        // Same as `to_selection_dialog` (Shared): re-derive widget-level focus from `d.focus`
        // (Input mode's own default, `Label`) instead of leaving whatever cloned field happened
        // to be focused in Selection mode.
        SetFocus::set_focused(&mut d, true);
        d
    }

    fn is_boolean_kind(&self) -> bool {
        matches!(
            self.kind.state.get_value().0,
            Kind::Coil | Kind::DiscreteInput
        )
    }

    fn validate(&self) -> Result<(), String> {
        if let ValidateResult::Error(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
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
        Ok(())
    }

    /// Validate and produce `(name, MonitorRegisterDef)`; see
    /// `EditInterpretationDialog::apply`'s own doc comment (Shared).
    pub fn apply(&self) -> Result<(String, MonitorRegisterDef), String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        let description = self.description.state.input().trim().to_string();
        let address = parse_address(self.address.state.input())?;
        let address = match address {
            ferrowl_codec::Address::Fixed(a) => Some(a),
            ferrowl_codec::Address::Virtual => None,
        };
        let is_virtual = address.is_none();
        let kind = self.kind.state.get_value().0.clone();

        let (value_type, endian, word_order, resolution, bitmask, alignment, length) =
            if self.is_boolean_kind() {
                (
                    crate::config::device::ValueType::U16,
                    EndianCfg::Big,
                    WordOrderCfg::Normal,
                    1.0,
                    None,
                    AlignmentCfg::Left,
                    1usize,
                )
            } else {
                match self.value_type.state.get_value() {
                    ValueType::Number => {
                        let selected = self.number_format.state.get_value();
                        let endian = self.number_endian.state.get_value().0.clone();
                        let word_order = self.number_word_order.state.get_value().0;
                        let resolution = self
                            .number_resolution
                            .state
                            .input()
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| "Resolution must be a number.".to_string())?;
                        let bitfield = if is_integer_format(&selected.0) {
                            parse_bitmask(self.number_bitmask.state.input())
                                .map_err(|e| format!("Bitmask {e}."))?
                        } else {
                            BitField::default()
                        };
                        let bitmask = if bitfield.is_full() {
                            None
                        } else {
                            Some(format!("0x{:X}", bitfield.mask))
                        };
                        (
                            value_type_from_format(&selected.0),
                            endian_cfg(&endian),
                            word_order_cfg(word_order),
                            resolution,
                            bitmask,
                            AlignmentCfg::Left,
                            1,
                        )
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
                        (
                            crate::config::device::ValueType::Ascii,
                            EndianCfg::Big,
                            WordOrderCfg::Normal,
                            1.0,
                            None,
                            alignment_cfg(alignment),
                            width,
                        )
                    }
                }
            };

        let def = MonitorRegisterDef {
            slave_id: 0,
            kind,
            address,
            is_virtual,
            value_type,
            endian,
            word_order,
            resolution,
            bitmask,
            length,
            alignment,
            values: self.value.state.values().clone(),
            description,
            default: None,
        };
        Ok((name, def))
    }

    pub fn open_add_dialog(&mut self) {
        self.add_dialog = Some(AddNamedValueDialog::new());
    }

    pub fn confirm_add_dialog(&mut self) {
        let result = self.add_dialog.as_ref().map(|d| d.apply());
        match result {
            Some(Ok(nv)) => {
                self.value.state.values_mut().push(nv);
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

    /// Open the delete-confirmation popup (MB-R-148), named after the dialog's current label
    /// input — see `EditInterpretationDialog::open_confirm_delete` (Shared).
    pub fn open_confirm_delete(&mut self) {
        let name = self.label.state.input().to_string();
        self.confirm_delete = Some(ConfirmDeleteDialog::new(&name));
    }

    /// Delete the `value`-selected alias immediately (no confirm popup — this deletes one alias,
    /// not the interpretation), mirroring `EditSelectionDialog::delete_selected` minus its
    /// `default_value` bookkeeping (this dialog has no default-value field, Shared).
    pub fn delete_selected_named_value(&mut self) {
        let idx = self.value.state.selection();
        let vals = self.value.state.values_mut();
        if vals.is_empty() {
            return;
        }
        vals.remove(idx);
        if vals.is_empty() {
            self.value.state.set_selection(0);
            self.focus_previous();
        } else {
            let new_idx = if idx >= vals.len() {
                vals.len() - 1
            } else {
                idx
            };
            self.value.state.set_selection(new_idx);
        }
    }

    pub fn handle_space(&mut self) {
        match self.focus {
            EditInterpretationSelectionDialogFocus::AddButton => self.open_add_dialog(),
            EditInterpretationSelectionDialogFocus::DeleteValueButton => {
                self.delete_selected_named_value()
            }
            EditInterpretationSelectionDialogFocus::DeleteButton => self.open_confirm_delete(),
            _ => {
                let _ = HandleEvents::handle_events(self, KeyModifiers::NONE, KeyCode::Char(' '));
            }
        }
    }

    pub fn is_confirm_button_focused(&self) -> bool {
        matches!(
            self.focus,
            EditInterpretationSelectionDialogFocus::ConfirmButton
        )
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.validate() {
            Ok(()) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(76), Constraint::Min(1)])
                .areas(area);
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(30),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg))
            .title_alignment(HorizontalAlignment::Center)
            .title(if self.deletable {
                "Edit interpretation"
            } else {
                "Add interpretation"
            });
        let dialog_box = vertical_layout[1];
        let block_inner = block.inner(dialog_box);
        let area = block_inner.inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, dialog_box, buf);
        block.render(dialog_box, buf);

        let rows: [Rect; 10] = Layout::vertical([
            Constraint::Length(3), // 0 label
            Constraint::Length(3), // 1 description
            Constraint::Length(3), // 2 address
            Constraint::Length(3), // 3 kind + type (value_type / boolean_type)
            Constraint::Length(3), // 4 Number/Text-conditional fields
            Constraint::Length(3), // 5 value + add_button + delete_value_button
            Constraint::Length(3), // 6 confirm_button [+ delete_button when deletable]
            Constraint::Length(3), // 7 error
            Constraint::Length(1), // 8 keybind0
            Constraint::Length(1), // 9 keybind1
        ])
        .areas(area);

        StatefulWidget::render(&self.label.widget, rows[0], buf, &mut self.label.state);
        StatefulWidget::render(
            &self.description.widget,
            rows[1],
            buf,
            &mut self.description.state,
        );
        StatefulWidget::render(&self.address.widget, rows[2], buf, &mut self.address.state);
        let kind_type: [Rect; 2] =
            Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(rows[3]);
        StatefulWidget::render(&self.kind.widget, kind_type[0], buf, &mut self.kind.state);
        if self.is_boolean_kind() {
            StatefulWidget::render(
                &self.boolean_type.widget,
                kind_type[1],
                buf,
                &mut self.boolean_type.state,
            );
        } else {
            StatefulWidget::render(
                &self.value_type.widget,
                kind_type[1],
                buf,
                &mut self.value_type.state,
            );
            match self.value_type.state.values()[self.value_type.state.selection()] {
                ValueType::Number => {
                    let integer = is_integer_format(&self.number_format.get_value().0);
                    let multi = is_multi_register_format(&self.number_format.get_value().0);
                    let columns = 3 + multi as usize + integer as usize;
                    let cells =
                        Layout::horizontal(vec![Constraint::Min(1); columns]).split(rows[4]);

                    let mut col = 0;
                    StatefulWidget::render(
                        &self.number_format.widget,
                        cells[col],
                        buf,
                        &mut self.number_format.state,
                    );
                    col += 1;

                    StatefulWidget::render(
                        &self.number_endian.widget,
                        cells[col],
                        buf,
                        &mut self.number_endian.state,
                    );
                    col += 1;

                    if multi {
                        StatefulWidget::render(
                            &self.number_word_order.widget,
                            cells[col],
                            buf,
                            &mut self.number_word_order.state,
                        );
                        col += 1;
                    }

                    StatefulWidget::render(
                        &self.number_resolution.widget,
                        cells[col],
                        buf,
                        &mut self.number_resolution.state,
                    );
                    col += 1;

                    if integer {
                        StatefulWidget::render(
                            &self.number_bitmask.widget,
                            cells[col],
                            buf,
                            &mut self.number_bitmask.state,
                        );
                    }
                }
                ValueType::Text => {
                    let cells: [Rect; 2] =
                        Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(rows[4]);
                    StatefulWidget::render(
                        &self.text_alignment.widget,
                        cells[0],
                        buf,
                        &mut self.text_alignment.state,
                    );
                    StatefulWidget::render(
                        &self.text_width.widget,
                        cells[1],
                        buf,
                        &mut self.text_width.state,
                    );
                }
            }
        }

        let value_row: [Rect; 4] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(18),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .areas(rows[5]);
        StatefulWidget::render(&self.value.widget, value_row[0], buf, &mut self.value.state);
        StatefulWidget::render(
            &self.delete_value_button.widget,
            value_row[2],
            buf,
            &mut self.delete_value_button.state,
        );
        StatefulWidget::render(
            &self.add_button.widget,
            value_row[1],
            buf,
            &mut self.add_button.state,
        );

        if self.deletable {
            let confirm_delete: [Rect; 2] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(rows[6]);
            StatefulWidget::render(
                &self.confirm_button.widget,
                confirm_delete[0],
                buf,
                &mut self.confirm_button.state,
            );
            StatefulWidget::render(
                &self.delete_button.widget,
                confirm_delete[1],
                buf,
                &mut self.delete_button.state,
            );
        } else {
            StatefulWidget::render(
                &self.confirm_button.widget,
                rows[6],
                buf,
                &mut self.confirm_button.state,
            );
        }

        if !self.error.state.is_empty() {
            StatefulWidget::render(&self.error.widget, rows[7], buf, &mut self.error.state);
        }
        StatefulWidget::render(
            &self.keybinds[0].widget,
            rows[8],
            buf,
            &mut self.keybinds[0].state,
        );
        StatefulWidget::render(
            &self.keybinds[1].widget,
            rows[9],
            buf,
            &mut self.keybinds[1].state,
        );

        if let Some(dialog) = self.add_dialog.as_mut() {
            dialog.render(area, buf);
        }
        if let Some(confirm) = self.confirm_delete.as_mut() {
            confirm.render(area, buf);
        }
    }
}

fn endian_cfg(e: &RegisterEndian) -> EndianCfg {
    match e {
        RegisterEndian::Big => EndianCfg::Big,
        RegisterEndian::Little => EndianCfg::Little,
    }
}

fn word_order_cfg(w: RegisterWordOrder) -> WordOrderCfg {
    match w {
        RegisterWordOrder::Normal => WordOrderCfg::Normal,
        RegisterWordOrder::Reversed => WordOrderCfg::Reversed,
    }
}

fn alignment_cfg(a: ferrowl_codec::Alignment) -> AlignmentCfg {
    match a {
        ferrowl_codec::Alignment::Left => AlignmentCfg::Left,
        ferrowl_codec::Alignment::Right => AlignmentCfg::Right,
    }
}

fn value_type_from_format(format: &RegisterFormat) -> crate::config::device::ValueType {
    use crate::config::device::ValueType as VT;
    match format {
        RegisterFormat::U8(_) => VT::U8,
        RegisterFormat::U16(_) => VT::U16,
        RegisterFormat::U32(_) => VT::U32,
        RegisterFormat::U64(_) => VT::U64,
        RegisterFormat::U128(_) => VT::U128,
        RegisterFormat::I8(_) => VT::I8,
        RegisterFormat::I16(_) => VT::I16,
        RegisterFormat::I32(_) => VT::I32,
        RegisterFormat::I64(_) => VT::I64,
        RegisterFormat::I128(_) => VT::I128,
        RegisterFormat::F32(_) => VT::F32,
        RegisterFormat::F64(_) => VT::F64,
        RegisterFormat::Ascii(_) => VT::Ascii,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::modbus::dialog::{kind_index, set_input};

    /// The crux of the add/edit unification: `EditInterpretationDialog::new()` (used by `:add`,
    /// UI-R-061) is not deletable, while `from_interpretation` (used by the MB-R-148 edit-on-a-
    /// row dialog, UI-R-064) is — mirroring `EditInputDialog::new()`/`from_register`'s own
    /// `deletable` split (Shared).
    #[test]
    fn ut_new_is_not_deletable_and_from_interpretation_is() {
        assert!(!EditInterpretationDialog::new().deletable);
        assert!(EditInterpretationDialog::from_interpretation("power", &sample_def()).deletable);
    }

    /// UI-R-061 — `apply()` over a filled-in `:add` dialog (`new()`, not deletable) builds a
    /// `MonitorRegisterDef` with the expected kind/address/value_type, and no access/value/
    /// default field exists on the struct at all (proven by the struct definition itself
    /// compiling without them).
    #[test]
    fn ut_add_interpretation_dialog_apply_builds_monitor_register_def() {
        let mut dialog = EditInterpretationDialog::new();
        set_input(&mut dialog.label, "power");
        set_input(&mut dialog.description, "Active power");
        set_input(&mut dialog.address, "10");
        dialog
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        dialog.number_format.state.set_selection(1); // U16, per widgets::format_options() order

        let (name, def) = dialog.apply().expect("valid dialog applies");
        assert_eq!(name, "power");
        assert_eq!(def.description, "Active power");
        assert_eq!(def.address, Some(10));
        assert_eq!(def.kind, Kind::HoldingRegister);
        assert_eq!(def.value_type, crate::config::device::ValueType::U16);
        assert_eq!(def.slave_id, 0);
    }

    /// Regression — selecting Number (or Text) value type must show its associated fields
    /// (Format/Endian/Resolution/[Order]/[Bitmask], or Alignment/Width); previously the struct
    /// held these fields and `apply()` already read them, but `render()` never drew any of them
    /// at all, so the user had no way to see or edit a Number/Text interpretation's format.
    #[test]
    fn ut_render_shows_number_format_fields_when_number_type_selected() {
        let mut dialog = EditInterpretationDialog::new();
        dialog
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        // ValueType::Number is index 0 per EditInterpretationDialog::new()'s selection order.
        dialog.value_type.state.set_selection(0);

        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        for field in ["Format", "Endian", "Resolution"] {
            assert!(text.contains(field), "missing field '{field}':\n{text}");
        }
    }

    /// Same regression, for the Text branch's Alignment/Width fields.
    #[test]
    fn ut_render_shows_text_format_fields_when_text_type_selected() {
        let mut dialog = EditInterpretationDialog::new();
        dialog
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        // ValueType::Text is index 1 per EditInterpretationDialog::new()'s selection order.
        dialog.value_type.state.set_selection(1);

        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        for field in ["Alignment", "Width"] {
            assert!(text.contains(field), "missing field '{field}':\n{text}");
        }
    }

    /// Same class of bug as `setup_dialog::ut_render_hides_error_box_when_valid_and_shows_it_when_invalid`
    /// (Shared): the error box must not draw at all while the dialog validates cleanly, only
    /// once it doesn't.
    #[test]
    fn ut_render_hides_error_box_when_valid_and_shows_it_when_invalid() {
        let area = Rect::new(0, 0, 120, 40);

        let mut valid = EditInterpretationDialog::new();
        set_input(&mut valid.label, "power");
        set_input(&mut valid.description, "Active power");
        set_input(&mut valid.address, "10");
        valid
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        valid.number_format.state.set_selection(1); // U16
        let mut buf = Buffer::empty(area);
        valid.render(area, &mut buf);
        assert!(
            !buffer_text(&buf).contains("Error"),
            "no error box should be drawn while the dialog is valid"
        );

        let mut invalid = EditInterpretationDialog::new(); // empty label -> invalid
        let mut buf = Buffer::empty(area);
        invalid.render(area, &mut buf);
        assert!(
            buffer_text(&buf).contains("Error"),
            "the error box must be visible once the dialog is invalid"
        );
    }

    /// UI-R-061 — the `:add` dialog (`new()`, `deletable == false`) shows Confirm alone,
    /// full-width, and no DELETE button (nothing exists yet to delete), with the "Add
    /// interpretation" title — mirrors `EditInputDialog::render`'s own `deletable` branch
    /// (Shared).
    #[test]
    fn ut_add_dialog_render_shows_confirm_alone_and_add_title() {
        let area = Rect::new(0, 0, 120, 40);
        let mut dialog = EditInterpretationDialog::new();
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(
            text.contains("Add interpretation"),
            "missing Add title:\n{text}"
        );
        assert!(text.contains("CONFIRM"), "missing Confirm button:\n{text}");
        assert!(
            !text.contains("DELETE"),
            "the :add dialog must not show a Delete button:\n{text}"
        );
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// The add-named-value button reads "ADD ALIAS", not "ADD PREDEFINED".
    #[test]
    fn ut_add_button_label_is_add_alias() {
        let area = Rect::new(0, 0, 120, 40);
        let mut dialog = EditInterpretationDialog::new();
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("ADD ALIAS"), "missing button label:\n{text}");
        assert!(
            !text.contains("PREDEFINED"),
            "stale label still present:\n{text}"
        );
    }

    /// Regression — the dialog's border must be styled like every other setup/edit popup in
    /// the crate (blue border, themed background), not the terminal's plain default; previously
    /// `Block::bordered()` had no `.style(...)` at all.
    #[test]
    fn ut_render_styles_the_border_with_the_theme_colors() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut dialog = EditInterpretationDialog::new();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                dialog.render(area, frame.buffer_mut());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let [_, hcenter, _] = ratatui::layout::Layout::horizontal([
            Constraint::Min(1),
            Constraint::Max(76),
            Constraint::Min(1),
        ])
        .areas(area_full(buf));
        let [_, vcenter, _] = ratatui::layout::Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(30),
            Constraint::Min(1),
        ])
        .areas(hcenter);
        assert_eq!(
            buf[(vcenter.x, vcenter.y)].fg,
            COLOR_SCHEME.hi,
            "dialog border must use the theme highlight color"
        );
        assert_eq!(
            buf[(vcenter.x, vcenter.y)].bg,
            COLOR_SCHEME.bg,
            "dialog border must use the theme background color"
        );
    }

    fn area_full(buf: &ratatui::buffer::Buffer) -> Rect {
        buf.area
    }

    /// Regression guard: `AddNamedValueDialog` still behaves as `dialog/add_value.rs`'s own
    /// tests already prove — nothing about wiring it into this dialog broke it.
    #[test]
    fn ut_add_named_value_flow_reused_unchanged() {
        let mut dialog = EditInterpretationDialog::new();
        dialog.open_add_dialog();
        assert!(dialog.add_dialog.is_some());
        {
            let sub = dialog.add_dialog.as_mut().unwrap();
            set_input(&mut sub.label, "on");
            set_input(&mut sub.value, "1");
        }
        dialog.confirm_add_dialog();
        assert!(dialog.add_dialog.is_none());
        assert_eq!(dialog.pending_named_values.len(), 1);
        assert_eq!(dialog.pending_named_values[0].name, "on");
    }

    /// Mirrors the `:add` counterpart above (`ut_add_dialog_render_shows_confirm_alone_and_add_title`):
    /// once `:add` mode-switches into `EditInterpretationSelectionDialog` (via `to_selection_dialog`,
    /// carrying `deletable == false` across), the Selection-mode dialog must also show Confirm
    /// alone, no DELETE.
    #[test]
    fn ut_selection_dialog_not_deletable_by_default_shows_confirm_alone() {
        let area = Rect::new(0, 0, 120, 40);
        let mut dialog = EditInterpretationSelectionDialog::new(vec![NamedValue {
            name: "on".to_string(),
            value: crate::config::device::Scalar::Int(1),
        }]);
        assert!(!dialog.deletable, "new() must default to not deletable");
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(
            text.contains("Add interpretation"),
            "missing Add title:\n{text}"
        );
        assert!(text.contains("CONFIRM"), "missing Confirm button:\n{text}");
        assert!(
            !text.contains("DELETE"),
            "a non-deletable Selection-mode dialog must not show a Delete button:\n{text}"
        );
    }

    fn sample_def() -> MonitorRegisterDef {
        MonitorRegisterDef {
            slave_id: 3,
            kind: Kind::HoldingRegister,
            address: Some(10),
            is_virtual: false,
            value_type: crate::config::device::ValueType::U16,
            endian: EndianCfg::Big,
            word_order: WordOrderCfg::Normal,
            resolution: 1.0,
            bitmask: None,
            length: 1,
            alignment: AlignmentCfg::Left,
            values: vec![],
            description: "Active power draw".to_string(),
            default: None,
        }
    }

    /// MB-R-148 — `from_interpretation` prefills every field from the existing row: label,
    /// description, address, kind, and (for a Number/HoldingRegister row) format/endian/
    /// resolution.
    #[test]
    fn ut_edit_interpretation_dialog_prefills_from_existing_row() {
        let def = sample_def();
        let dialog = EditInterpretationDialog::from_interpretation("power", &def);
        assert_eq!(dialog.label.state.input(), "power");
        assert_eq!(dialog.description.state.input(), "Active power draw");
        assert_eq!(dialog.address.state.input(), "10");
        assert_eq!(
            dialog.kind.state.selection(),
            kind_index(&Kind::HoldingRegister)
        );
        assert_eq!(dialog.value_type.state.selection(), 0); // Number
        assert_eq!(dialog.number_resolution.state.input(), "1");
    }

    /// MB-R-148 — `apply()` over a prefilled, unmodified dialog round-trips back to an
    /// equivalent `MonitorRegisterDef` (`slave_id` is always reset to 0: the caller scopes it).
    #[test]
    fn ut_edit_interpretation_dialog_apply_round_trips_unmodified() {
        let def = sample_def();
        let dialog = EditInterpretationDialog::from_interpretation("power", &def);
        let (name, applied) = dialog.apply().expect("valid dialog applies");
        assert_eq!(name, "power");
        assert_eq!(applied.description, def.description);
        assert_eq!(applied.address, def.address);
        assert_eq!(applied.kind, def.kind);
        assert_eq!(applied.value_type, def.value_type);
        assert_eq!(applied.slave_id, 0);
    }

    /// MB-R-148 — pressing Space on the focused Delete button opens the confirmation popup, not
    /// an immediate delete.
    #[test]
    fn ut_delete_button_space_opens_confirm_delete_not_immediate_delete() {
        let mut dialog = EditInterpretationDialog::from_interpretation("power", &sample_def());
        assert!(dialog.confirm_delete.is_none());
        dialog.focus = EditInterpretationDialogFocus::DeleteButton;
        dialog.handle_space();
        assert!(dialog.confirm_delete.is_some());
    }

    /// Manual-exercise fix (item 4), applied here too for parity — `EditInterpretationDialog`
    /// has the identical error-box-always-drawn bug as `AddInterpretationDialog` (Shared).
    #[test]
    fn ut_edit_interpretation_render_hides_error_box_when_valid_and_shows_it_when_invalid() {
        let area = Rect::new(0, 0, 120, 40);

        let mut valid = EditInterpretationDialog::from_interpretation("power", &sample_def());
        let mut buf = Buffer::empty(area);
        valid.render(area, &mut buf);
        assert!(
            !buffer_text(&buf).contains("Error"),
            "no error box should be drawn while the dialog is valid"
        );

        let mut invalid = EditInterpretationDialog::from_interpretation("power", &sample_def());
        set_input(&mut invalid.address, ""); // empty address fails `parse_address` -> invalid
        let mut buf = Buffer::empty(area);
        invalid.render(area, &mut buf);
        assert!(
            buffer_text(&buf).contains("Error"),
            "the error box must be visible once the dialog is invalid"
        );
    }

    /// Manual-exercise fix (item 3) — while `pending_named_values` is empty,
    /// `EditInterpretationDialog` shows a single full-width "ADD ALIAS" button, not an inline
    /// list (mirrors `EditInputDialog`, Shared).
    #[test]
    fn ut_edit_interpretation_render_shows_add_button_when_empty() {
        let area = Rect::new(0, 0, 120, 40);
        let mut empty = EditInterpretationDialog::from_interpretation("power", &sample_def());
        let mut buf = Buffer::empty(area);
        empty.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("ADD ALIAS"), "missing add button:\n{text}");
        assert!(
            !text.contains("No predefined values"),
            "the old inline-list placeholder must be gone:\n{text}"
        );
    }

    /// Manual-exercise fix (item 3) — `to_selection_dialog` carries every shared field's state
    /// (here: the label) plus `pending_named_values` over into a fresh
    /// `EditInterpretationSelectionDialog`, mirroring `EditInputDialog::to_edit_selection_dialog`
    /// (Shared).
    #[test]
    fn ut_edit_interpretation_to_selection_dialog_carries_state_and_values() {
        let mut dialog = EditInterpretationDialog::from_interpretation("power", &sample_def());
        dialog.pending_named_values.push(NamedValue {
            name: "on".to_string(),
            value: crate::config::device::Scalar::Int(1),
        });
        dialog.pending_named_values.push(NamedValue {
            name: "off".to_string(),
            value: crate::config::device::Scalar::Int(0),
        });

        let selection = dialog.to_selection_dialog();

        assert_eq!(selection.label.state.input(), dialog.label.state.input());
        assert_eq!(selection.value.state.values().len(), 2);
        assert_eq!(selection.value.state.values()[0].name, "on");
        assert_eq!(selection.value.state.values()[1].name, "off");
        assert!(
            selection.deletable,
            "deletable must carry over from the (deletable, from_interpretation) source dialog"
        );
    }

    /// Manual-exercise fix (item 3) — `EditInterpretationSelectionDialog::delete_selected_named_value`
    /// removes exactly the `value`-selected entry, same shape as
    /// `EditSelectionDialog::delete_selected` minus its `default_value` bookkeeping (Shared).
    #[test]
    fn ut_edit_interpretation_selection_dialog_delete_removes_selected_named_value() {
        let values = vec![
            NamedValue {
                name: "on".to_string(),
                value: crate::config::device::Scalar::Int(1),
            },
            NamedValue {
                name: "off".to_string(),
                value: crate::config::device::Scalar::Int(0),
            },
        ];
        let mut dialog = EditInterpretationSelectionDialog::new(values);
        dialog.value.state.set_selection(1); // "off"

        dialog.focus = EditInterpretationSelectionDialogFocus::DeleteValueButton;
        dialog.handle_space();

        assert_eq!(dialog.value.state.values().len(), 1);
        assert_eq!(dialog.value.state.values()[0].name, "on");
    }

    /// Manual-exercise fix (item 3) — once the alias list empties out (deleting the last one),
    /// `to_input_dialog` carries the shared state back into a fresh `EditInterpretationDialog`,
    /// mirroring `EditSelectionDialog::to_edit_input_dialog` (Shared).
    #[test]
    fn ut_edit_interpretation_selection_dialog_to_input_dialog_carries_state() {
        let values = vec![NamedValue {
            name: "on".to_string(),
            value: crate::config::device::Scalar::Int(1),
        }];
        let mut selection = EditInterpretationSelectionDialog::new(values);
        set_input(&mut selection.label, "power");

        let back = selection.to_input_dialog();

        assert_eq!(back.label.state.input(), "power");
        assert!(back.pending_named_values.is_empty());
        assert!(
            !back.deletable,
            "deletable must carry over from the (not deletable, new()) source dialog"
        );
    }

    /// `from_interpretation` starts focus on Address (`address.state.focused = true`, not
    /// Label's own default-`true`); `to_selection_dialog` must not carry that stale
    /// `focused = true` over into the fresh `EditInterpretationSelectionDialog` alongside its
    /// own default `value.state.focused = true`. Exactly one field's widget-level `focused`
    /// must be true after the swap, matching `d.focus`.
    #[test]
    fn ut_edit_interpretation_to_selection_dialog_focuses_exactly_one_field() {
        let mut def = sample_def();
        def.values = vec![NamedValue {
            name: "on".to_string(),
            value: crate::config::device::Scalar::Int(1),
        }];
        let dialog = EditInterpretationDialog::from_interpretation("power", &def);
        assert!(
            dialog.address.state.focused(),
            "sanity: from_interpretation starts focus on Address"
        );

        let selection = dialog.to_selection_dialog();

        assert!(
            !selection.address.state.focused(),
            "the carried-over Address field must not still show focused in Selection mode"
        );
        assert!(
            selection.value.state.focused(),
            "Selection mode's own default focus (Value) must be the only one focused"
        );
        assert_eq!(
            selection.focus,
            EditInterpretationSelectionDialogFocus::Value
        );
    }

    /// The reverse direction of the above: converting back from `Selection` to `Input` mode
    /// (`to_input_dialog`, once the last alias is deleted) must leave exactly `Label` (its own
    /// default focus) focused, not whatever field happened to be focused in `Selection` mode.
    #[test]
    fn ut_edit_interpretation_selection_to_input_dialog_focuses_exactly_one_field() {
        let mut selection = EditInterpretationSelectionDialog::new(vec![NamedValue {
            name: "on".to_string(),
            value: crate::config::device::Scalar::Int(1),
        }]);
        selection.focus_next(); // Value -> AddButton
        assert!(
            !selection.label.state.focused(),
            "sanity: Label is not the Selection dialog's focused field here"
        );

        let back = selection.to_input_dialog();

        assert!(
            back.label.state.focused(),
            "Input mode's own default focus (Label) must be the only one focused"
        );
        assert_eq!(back.focus, EditInterpretationDialogFocus::Label);
    }

    /// `EditInterpretationDialog`'s declared dialog-box height must match what its own 10-row
    /// `rows` array actually needs (`3*8 + 1*2 = 26` interior rows, `+4` border/margin = `30`):
    /// undersizing it lets the ratatui layout solver silently shrink some of the `Length(3)`
    /// rows down to 2 rows each to fit, leaving those rows' *content* line entirely unrendered
    /// (border-border, no content row in between) — not merely "cramped", but the field's value
    /// invisible outright.
    #[test]
    fn ut_edit_interpretation_render_does_not_shrink_rows_below_declared_height() {
        let mut dialog = EditInterpretationDialog::from_interpretation("power", &sample_def());
        set_input(&mut dialog.number_resolution, "2.75");
        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);

        assert!(
            text.contains("Active power draw"),
            "the description's own content line must render, not just its border:\n{text}"
        );
        assert!(
            text.contains("2.75"),
            "the Resolution field's own content line must render, not just its border:\n{text}"
        );
        assert!(
            text.contains("CONFIRM") && text.contains("DELETE"),
            "the Confirm/Delete buttons' own content line must render, not just their border:\n{text}"
        );
    }

    /// Same box-height-vs-row-sum requirement as `EditInterpretationDialog` (`Length(30)`
    /// needed by its own identical 10-row array), reached once the mode-switch opens this
    /// dialog instead (Shared).
    #[test]
    fn ut_edit_interpretation_selection_render_does_not_shrink_rows_below_declared_height() {
        let mut dialog = EditInterpretationSelectionDialog::new(vec![NamedValue {
            name: "on".to_string(),
            value: crate::config::device::Scalar::Int(1),
        }]);
        dialog.deletable = true; // reached via the MB-R-148 edit path, which is deletable
        set_input(&mut dialog.label, "power");
        set_input(&mut dialog.description, "Active power draw");
        set_input(&mut dialog.address, "10");
        dialog
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        set_input(&mut dialog.number_resolution, "2.75");
        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);

        assert!(
            text.contains("Active power draw"),
            "the description's own content line must render, not just its border:\n{text}"
        );
        assert!(
            text.contains("2.75"),
            "the Resolution field's own content line must render, not just its border:\n{text}"
        );
        assert!(
            text.contains("CONFIRM") && text.contains("DELETE"),
            "the Confirm/Delete buttons' own content line must render, not just their border:\n{text}"
        );
    }
}
