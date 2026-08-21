//! `AddInterpretationDialog` (UI-R-061): a purpose-built `:add`/`:a` dialog for a monitor
//! module, producing a [`MonitorRegisterDef`] (MB-R-145) scoped to the view's currently
//! selected unit id (the dialog itself never asks for a slave id).
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
    traits::HandleEvents,
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
    AddNamedValueDialog, Alignment, Endian, Format, KindOption, ValueType, WordOrder,
    is_integer_format, is_multi_register_format, parse_address, parse_bitmask, widgets,
};

#[focusable]
#[derive(Builder, Debug, Focus)]
pub struct AddInterpretationDialog {
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
    #[focus]
    pub add_button: Widget<ButtonState, Button>,
    #[focus]
    pub confirm_button: Widget<ButtonState, Button>,
    pub error: Widget<String, Text>,
    pub keybinds: [Widget<String, Text>; 2],
    #[builder(default)]
    pub add_dialog: Option<AddNamedValueDialog>,
    #[builder(default)]
    pub pending_named_values: Vec<NamedValue>,
}

impl AddInterpretationDialog {
    pub fn new() -> Self {
        let mut label = widgets::input::<NonEmpty>("Label", "Custom label...");
        ferrowl_ui::traits::SetFocus::set_focused(&mut label.state, true);

        AddInterpretationDialogBuilder::default()
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
            .add_button(widgets::button("ADD PREDEFINED", 1))
            .confirm_button(widgets::button("CONFIRM", 1))
            .error(widgets::error_text())
            .keybinds([
                widgets::keybind("<Space>: press button | <C-f>: fill value | <Tab>: next"),
                widgets::keybind("<Esc>: close | <Enter>: confirm / newline"),
            ])
            .focus(AddInterpretationDialogFocus::Label)
            .build()
            .expect("all AddInterpretationDialog fields are set")
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
    /// (`0`) — the caller (the view) sets it to the currently selected unit id (UI-R-061).
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

    pub fn handle_space(&mut self) {
        if matches!(self.focus, AddInterpretationDialogFocus::AddButton) {
            self.open_add_dialog();
        } else {
            let _ = HandleEvents::handle_events(self, KeyModifiers::NONE, KeyCode::Char(' '));
        }
    }

    pub fn is_confirm_button_focused(&self) -> bool {
        matches!(self.focus, AddInterpretationDialogFocus::ConfirmButton)
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
            Constraint::Length(33),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg))
            .title_alignment(HorizontalAlignment::Center)
            .title("Add interpretation");
        let dialog_box = vertical_layout[1];
        let block_inner = block.inner(dialog_box);
        let area = block_inner.inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, dialog_box, buf);
        block.render(dialog_box, buf);

        let rows: [Rect; 11] = Layout::vertical([
            Constraint::Length(3), // 0 label
            Constraint::Length(3), // 1 description
            Constraint::Length(3), // 2 address
            Constraint::Length(3), // 3 kind
            Constraint::Length(3), // 4 type (value_type / boolean_type)
            Constraint::Length(3), // 5 Number/Text-conditional fields
            Constraint::Length(3), // 6 add_button
            Constraint::Length(3), // 7 confirm_button
            Constraint::Length(3), // 8 error
            Constraint::Length(1), // 9 keybind0
            Constraint::Length(1), // 10 keybind1
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
        StatefulWidget::render(&self.kind.widget, rows[3], buf, &mut self.kind.state);
        if self.is_boolean_kind() {
            StatefulWidget::render(
                &self.boolean_type.widget,
                rows[4],
                buf,
                &mut self.boolean_type.state,
            );
        } else {
            StatefulWidget::render(
                &self.value_type.widget,
                rows[4],
                buf,
                &mut self.value_type.state,
            );
            // Mirrors EditInputDialog's render.rs Number/Text branch — the fields that were
            // missing entirely here (struct fields existed and `apply()` already read them,
            // but render() never drew them, so the user had no way to see or edit them).
            match self.value_type.state.values()[self.value_type.state.selection()] {
                ValueType::Number => {
                    let integer = is_integer_format(&self.number_format.get_value().0);
                    let multi = is_multi_register_format(&self.number_format.get_value().0);
                    let columns = 3 + multi as usize + integer as usize;
                    let cells =
                        Layout::horizontal(vec![Constraint::Min(1); columns]).split(rows[5]);

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
                        Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(rows[5]);
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
        StatefulWidget::render(
            &self.add_button.widget,
            rows[6],
            buf,
            &mut self.add_button.state,
        );
        StatefulWidget::render(
            &self.confirm_button.widget,
            rows[7],
            buf,
            &mut self.confirm_button.state,
        );
        StatefulWidget::render(&self.error.widget, rows[8], buf, &mut self.error.state);
        StatefulWidget::render(
            &self.keybinds[0].widget,
            rows[9],
            buf,
            &mut self.keybinds[0].state,
        );
        StatefulWidget::render(
            &self.keybinds[1].widget,
            rows[10],
            buf,
            &mut self.keybinds[1].state,
        );

        if let Some(dialog) = self.add_dialog.as_mut() {
            dialog.render(area, buf);
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

    /// UI-R-061 — `apply()` over a filled-in dialog builds a `MonitorRegisterDef` with the
    /// expected kind/address/value_type, and no access/value/default field exists on the
    /// struct at all (proven by the struct definition itself compiling without them).
    #[test]
    fn ut_add_interpretation_dialog_apply_builds_monitor_register_def() {
        let mut dialog = AddInterpretationDialog::new();
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
        let mut dialog = AddInterpretationDialog::new();
        dialog
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        // ValueType::Number is index 0 per AddInterpretationDialog::new()'s selection order.
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
        let mut dialog = AddInterpretationDialog::new();
        dialog
            .kind
            .state
            .set_selection(kind_index(&Kind::HoldingRegister));
        // ValueType::Text is index 1 per AddInterpretationDialog::new()'s selection order.
        dialog.value_type.state.set_selection(1);

        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        for field in ["Alignment", "Width"] {
            assert!(text.contains(field), "missing field '{field}':\n{text}");
        }
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

    /// Regression — the dialog's border must be styled like every other setup/edit popup in
    /// the crate (blue border, themed background), not the terminal's plain default; previously
    /// `Block::bordered()` had no `.style(...)` at all.
    #[test]
    fn ut_render_styles_the_border_with_the_theme_colors() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut dialog = AddInterpretationDialog::new();
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
            Constraint::Length(33),
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
        let mut dialog = AddInterpretationDialog::new();
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
}
