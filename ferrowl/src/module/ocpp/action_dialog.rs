//! Per-action send dialog shared by the OCPP client and server views.
//!
//! Instead of a free-form JSON editor, an action opens a property table (Name | Type | Value) built
//! from its [`ActionSpec`]. Enter on a row opens a typed value editor (text/number/bool/enum/now
//! timestamp). Rows prefill from a [`PropSource`] (observed state field, generated value, constant,
//! or empty). A "JSON" toggle switches to a raw editor prefilled from the current rows, for
//! payloads the table can't express; a "Send" button assembles the payload. The dialog only builds
//! the payload — the view validates it (`decode_call`) and sends it.
//!
//! Actions with no spec open straight in JSON mode (transitional, removed once every action has a
//! spec). Nested/abstracted actions supply a custom [`Assembler`] (see Stage 2).

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_lua::module::ValueType;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder, TableState,
        TableStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyleBuilder, SelectionStyleBuilder, TableStyleBuilder},
    traits::HandleEvents,
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, GetValue, InputField,
        InputFieldBuilder, Selection, SelectionBuilder, Table, TableBuilder, Widget,
    },
};
use ferrowl_ui_derive::TableEntry;
use ratatui::style::Style;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};
use serde_json::{Value, json};

/// Mint a fresh transaction id for `GeneratedTxId` prefills.
pub fn gen_tx_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Build a flat JSON object from coerced (name, value) pairs. Default [`ActionSpec::assemble`].
pub fn flat_object(pairs: &[(&'static str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        map.insert((*k).to_string(), v.clone());
    }
    Value::Object(map)
}

/// Assembles coerced property pairs into the final request payload.
pub type Assembler = fn(&[(&'static str, Value)]) -> Value;

/// Look up a coerced property value by wire name. Used by custom (nested) [`Assembler`]s to read
/// the flat table values they fold into a nested request. A required-but-empty field is present as
/// `Null`, which is treated as absent.
pub fn prop<'a>(pairs: &'a [(&'static str, Value)], name: &str) -> Option<&'a Value> {
    pairs
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
        .filter(|v| !v.is_null())
}

/// The input kind (and value-editor) for one property.
#[derive(Clone, Copy)]
pub enum PropKind {
    Text,
    Number,
    /// Boolean dropdown (used by Stage 2 nested actions).
    #[allow(dead_code)]
    Bool,
    /// A closed set of allowed string values, rendered as a dropdown.
    Enum(&'static [&'static str]),
    /// An RFC3339 timestamp; defaults to "now".
    Timestamp,
}

impl PropKind {
    fn label(&self) -> &'static str {
        match self {
            PropKind::Text => "text",
            PropKind::Number => "number",
            PropKind::Bool => "bool",
            PropKind::Enum(_) => "enum",
            PropKind::Timestamp => "timestamp",
        }
    }
}

/// Where a property's initial value comes from when the dialog opens.
#[derive(Clone, Copy)]
pub enum PropSource {
    /// An observed-state field, resolved via `OcppFields::get_field`.
    StateField(&'static str),
    /// A freshly generated transaction id (used by Stage 2 actions).
    #[allow(dead_code)]
    GeneratedTxId,
    /// The current time (RFC3339).
    Now,
    /// A fixed default string.
    Constant(&'static str),
    /// No prefill.
    Empty,
}

/// One property of an action: its wire name, input kind, prefill source, and whether it is optional.
pub struct PropSpec {
    pub name: &'static str,
    pub kind: PropKind,
    pub source: PropSource,
    pub optional: bool,
}

/// The full editing spec for one action: its properties and how to assemble them.
pub struct ActionSpec {
    pub props: &'static [PropSpec],
    pub assemble: Assembler,
    /// Whether the action has list/nested parts the flat table can't fully express (Stage 2).
    #[allow(dead_code)]
    pub complex: bool,
}

/// What the dialog asks the host view to do.
pub enum ActionResult {
    /// Close without sending.
    Close,
    /// Assemble succeeded; send this payload (the view validates via `decode_call`).
    Send(Value),
}

// --- Property table --------------------------------------------------------

#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = PropHeader)]
struct PropRow {
    #[column(name = "Property", min = 16, max = 28)]
    name: String,
    #[column(name = "Type", min = 8, max = 10)]
    kind: String,
    #[column(name = "Value", min = 10, max = 40)]
    value: String,
}

type PropTable = Widget<TableState<PropRow, 3>, Table<PropRow, PropHeader, 3>>;

/// A typed value editor for the selected row.
enum ValueEditor {
    Text(Widget<InputFieldState, InputField<String>>),
    Choice(Widget<SelectionState<String>, Selection<String>>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    /// The property table (table mode) or the JSON editor (JSON mode).
    Fields,
    /// The mode-toggle button.
    Toggle,
    /// The send button.
    Send,
}

/// One in-progress action send. `kinds` is parallel to the table rows (for coercion).
pub struct ActionDialog {
    pub name: String,
    /// `None` for spec-less actions (JSON-only).
    assemble: Option<Assembler>,
    /// Per-row (wire name, kind, optional) used for coercion/assembly.
    props: Vec<(&'static str, PropKind, bool)>,
    table: PropTable,
    json: Widget<CodeInputFieldState, CodeInputField>,
    json_mode: bool,
    editor: Option<ValueEditor>,
    toggle: Widget<ButtonState, Button>,
    send: Widget<ButtonState, Button>,
    focus: Focus,
    /// Compact rows (vertical row margin 0); `c` toggles it. Default is roomy (margin 1).
    compact: bool,
}

impl ActionDialog {
    /// Build a dialog from a spec, resolving each property's prefill source. `lookup` reads an
    /// observed-state field as a display string; `tx_id` mints a transaction id when needed.
    pub fn new(
        name: String,
        spec: &ActionSpec,
        lookup: impl Fn(&str) -> Option<String>,
        tx_id: impl Fn() -> String,
    ) -> Self {
        let mut rows = Vec::new();
        let mut props = Vec::new();
        for p in spec.props {
            let value = match p.source {
                PropSource::StateField(f) => lookup(f).unwrap_or_default(),
                PropSource::GeneratedTxId => tx_id(),
                PropSource::Now => crate::module::ocpp::client::backend::rfc3339_now(),
                PropSource::Constant(c) => c.to_string(),
                PropSource::Empty => String::new(),
            };
            rows.push(PropRow {
                name: p.name.to_string(),
                kind: p.kind.label().to_string(),
                value,
            });
            props.push((p.name, p.kind, p.optional));
        }
        let mut dlg = Self::scaffold(name, Some(spec.assemble), props);
        dlg.table.state.set_values(rows);
        dlg
    }

    /// Build a JSON-only dialog (no spec), prefilled with a template.
    pub fn json_only(name: String, template: &str) -> Self {
        let mut dlg = Self::scaffold(name, None, Vec::new());
        dlg.json.state.set_content(template);
        dlg.json_mode = true;
        dlg.toggle = button("Table");
        dlg
    }

    fn scaffold(
        name: String,
        assemble: Option<Assembler>,
        props: Vec<(&'static str, PropKind, bool)>,
    ) -> Self {
        Self {
            name,
            assemble,
            props,
            table: prop_table(),
            json: json_editor(),
            json_mode: false,
            editor: None,
            toggle: button("JSON"),
            send: button("Send"),
            focus: Focus::Fields,
            compact: false,
        }
    }

    /// Toggle compact rows in the property table (vertical row margin 1 ↔ 0).
    fn toggle_compact(&mut self) {
        self.compact = !self.compact;
        self.table.widget.set_row_margin(Margin {
            vertical: if self.compact { 0 } else { 1 },
            horizontal: 0,
        });
    }

    /// Coerce a string to JSON per its kind. `None` = omit (empty value).
    fn coerce(kind: PropKind, s: &str) -> Option<Value> {
        if s.is_empty() {
            return None;
        }
        match kind {
            PropKind::Number => {
                let f: f64 = s.parse().ok()?;
                if f.fract() == 0.0 && f.abs() < i64::MAX as f64 {
                    Some(json!(f as i64))
                } else {
                    Some(json!(f))
                }
            }
            PropKind::Bool => match s {
                "true" => Some(json!(true)),
                "false" => Some(json!(false)),
                _ => None,
            },
            PropKind::Text | PropKind::Enum(_) | PropKind::Timestamp => Some(json!(s)),
        }
    }

    /// Assemble the current rows into a payload via the spec's assembler.
    fn assemble_rows(&self) -> Value {
        let rows = self.table.state.values();
        let mut pairs: Vec<(&'static str, Value)> = Vec::new();
        for (i, (name, kind, optional)) in self.props.iter().enumerate() {
            let s = rows.get(i).map(|r| r.value.as_str()).unwrap_or("");
            match Self::coerce(*kind, s) {
                Some(v) => pairs.push((*name, v)),
                None if *optional => {}
                None => pairs.push((*name, Value::Null)),
            }
        }
        match self.assemble {
            Some(f) => f(&pairs),
            None => flat_object(&pairs),
        }
    }

    fn open_editor(&mut self) {
        let Some(i) = self.table.state.table_state().selected() else {
            return;
        };
        let Some((_, kind, _)) = self.props.get(i) else {
            return;
        };
        let current = self
            .table
            .state
            .values()
            .get(i)
            .map(|r| r.value.clone())
            .unwrap_or_default();
        self.editor = Some(match kind {
            PropKind::Bool => choice_editor(&["true", "false"], &current),
            PropKind::Enum(variants) => choice_editor(variants, &current),
            _ => {
                let mut field = text_editor();
                field.state.set_input(current.clone());
                field.state.set_cursor(current.chars().count());
                ValueEditor::Text(field)
            }
        });
    }

    /// Commit the open value editor into the selected row.
    fn commit_editor(&mut self) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        let Some(i) = self.table.state.table_state().selected() else {
            return;
        };
        let value = match editor {
            ValueEditor::Text(f) => f.state.get_value(),
            ValueEditor::Choice(s) => s.state.get_value(),
        };
        let mut rows = self.table.state.values().clone();
        if let Some(row) = rows.get_mut(i) {
            row.value = value;
        }
        self.table.state.set_values(rows);
    }

    fn toggle_mode(&mut self) {
        if self.json_mode {
            self.json_mode = false;
            self.toggle = button("JSON");
        } else {
            // Prefill the JSON editor from the current rows so nothing is lost.
            let assembled = self.assemble_rows();
            let text = serde_json::to_string_pretty(&assembled).unwrap_or_default();
            self.json.state.set_content(&text);
            self.json_mode = true;
            self.toggle = button("Table");
        }
        self.focus = Focus::Fields;
    }

    fn focus_next(&mut self) {
        // The Toggle button is hidden for JSON-only dialogs (no spec to switch back to).
        self.focus = match self.focus {
            Focus::Fields if self.assemble.is_none() => Focus::Send,
            Focus::Fields => Focus::Toggle,
            Focus::Toggle => Focus::Send,
            Focus::Send => Focus::Fields,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Focus::Fields => Focus::Send,
            Focus::Send if self.assemble.is_none() => Focus::Fields,
            Focus::Send => Focus::Toggle,
            Focus::Toggle => Focus::Fields,
        };
    }

    /// Handle a key. Returns an [`ActionResult`] when the host view must act.
    pub fn input(&mut self, modifiers: KeyModifiers, code: KeyCode) -> Option<ActionResult> {
        // The value editor captures keys while open.
        if let Some(editor) = self.editor.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.editor = None,
                (KeyModifiers::NONE, KeyCode::Enter) => self.commit_editor(),
                _ => match editor {
                    ValueEditor::Text(f) => {
                        let _ = f.state.handle_events(modifiers, code);
                    }
                    ValueEditor::Choice(s) => {
                        let _ = s.state.handle_events(modifiers, code);
                    }
                },
            }
            return None;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => return Some(ActionResult::Close),
            (KeyModifiers::NONE, KeyCode::Tab) => self.focus_next(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => self.focus_previous(),
            (KeyModifiers::NONE, KeyCode::Enter) => match self.focus {
                Focus::Toggle => self.toggle_mode(),
                Focus::Send => return Some(ActionResult::Send(self.payload())),
                Focus::Fields if self.json_mode => {
                    let _ = self.json.state.handle_events(modifiers, code);
                }
                Focus::Fields => self.open_editor(),
            },
            // `c` toggles compact rows while the property table is focused (JSON mode keeps `c` as
            // text input).
            (KeyModifiers::NONE, KeyCode::Char('c'))
                if !self.json_mode && self.focus == Focus::Fields =>
            {
                self.toggle_compact()
            }
            _ => {
                if self.focus == Focus::Fields {
                    if self.json_mode {
                        let _ = self.json.state.handle_events(modifiers, code);
                    } else {
                        let _ = self.table.state.handle_events(modifiers, code);
                    }
                }
            }
        }
        None
    }

    /// The payload to send: parsed JSON (JSON mode) or assembled rows (table mode).
    fn payload(&self) -> Value {
        if self.json_mode {
            serde_json::from_str(&self.json.state.content()).unwrap_or(Value::Null)
        } else {
            self.assemble_rows()
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let [_, hc, _] =
            Layout::horizontal([Constraint::Min(1), Constraint::Max(100), Constraint::Min(1)])
                .areas(area);
        let [_, vc, _] =
            Layout::vertical([Constraint::Min(1), Constraint::Max(50), Constraint::Min(1)])
                .areas(hc);

        UiWidget::render(&Clear, vc, buf);
        let block = Block::bordered()
            .style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg))
            .title_alignment(HorizontalAlignment::Center)
            .title(format!("Send {}", self.name));
        let inner = block.inner(vc);
        block.render(vc, buf);
        let inner = inner.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let [body, buttons] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(inner);

        if self.json_mode {
            self.json.state.set_focused(self.focus == Focus::Fields);
            StatefulWidget::render(&self.json.widget, body, buf, &mut self.json.state);
        } else {
            self.table.state.set_focused(self.focus == Focus::Fields);
            StatefulWidget::render(&self.table.widget, body, buf, &mut self.table.state);
        }

        // Buttons: [Toggle] [Send] (Toggle hidden for JSON-only dialogs).
        let show_toggle = self.assemble.is_some();
        if show_toggle {
            let [tb, sb] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(buttons);
            self.toggle.state.set_focused(self.focus == Focus::Toggle);
            self.send.state.set_focused(self.focus == Focus::Send);
            StatefulWidget::render(&self.toggle.widget, tb, buf, &mut self.toggle.state);
            StatefulWidget::render(&self.send.widget, sb, buf, &mut self.send.state);
        } else {
            self.send.state.set_focused(self.focus == Focus::Send);
            StatefulWidget::render(&self.send.widget, buttons, buf, &mut self.send.state);
        }

        if let Some(editor) = self.editor.as_mut() {
            let [_, mid, _] = Layout::vertical([
                Constraint::Percentage(40),
                Constraint::Length(3),
                Constraint::Percentage(40),
            ])
            .areas(vc.inner(Margin {
                vertical: 0,
                horizontal: 4,
            }));
            UiWidget::render(&Clear, mid, buf);
            match editor {
                ValueEditor::Text(f) => {
                    f.state.set_focused(true);
                    StatefulWidget::render(&f.widget, mid, buf, &mut f.state);
                }
                ValueEditor::Choice(s) => {
                    s.state.set_focused(true);
                    StatefulWidget::render(&s.widget, mid, buf, &mut s.state);
                }
            }
        }
    }
}

/// Display a `ValueType` field as a string for prefill.
pub fn value_to_string(v: ValueType) -> String {
    match v {
        ValueType::Int(i) => i.to_string(),
        ValueType::Float(f) => f.to_string(),
        ValueType::String(s) => s,
        ValueType::Bool(b) => b.to_string(),
        ValueType::Nil => "nil".into(),
    }
}

// --- Widget builders -------------------------------------------------------

fn border_style() -> Style {
    Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)
}

fn prop_table() -> PropTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Properties (Enter to edit, c: compact)".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn json_editor() -> Widget<CodeInputFieldState, CodeInputField> {
    Widget {
        state: CodeInputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .build()
            .unwrap(),
        widget: CodeInputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("JSON payload".into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn text_editor() -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(true)
            .disabled(false)
            .placeholder(Some("value (Enter to set)".to_string()))
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Value", HorizontalAlignment::Left).into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .unwrap(),
    }
}

/// A dropdown editor with `current` moved to the front so it is preselected.
fn choice_editor(variants: &[&str], current: &str) -> ValueEditor {
    let mut values: Vec<String> = Vec::new();
    if variants.contains(&current) {
        values.push(current.to_string());
    }
    for v in variants {
        if *v != current {
            values.push((*v).to_string());
        }
    }
    ValueEditor::Choice(Widget {
        state: SelectionStateBuilder::default()
            .focused(true)
            .values(values)
            .build()
            .unwrap(),
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Value", HorizontalAlignment::Left).into()))
            .style(
                SelectionStyleBuilder::default()
                    .general(border_style())
                    .focused(
                        Style::default()
                            .fg(COLOR_SCHEME.bg)
                            .bg(COLOR_SCHEME.hi)
                            .bold(),
                    )
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    })
}

fn button(label: &str) -> Widget<ButtonState, Button> {
    Widget {
        state: ButtonStateBuilder::default()
            .focused(false)
            .label(label.to_string())
            .disabled(false)
            .build()
            .unwrap(),
        widget: ButtonBuilder::default()
            .border_margin(Margin::new(1, 0))
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .style(ButtonStyle {
                general: border_style(),
                ..ButtonStyle::default()
            })
            .horizontal_alignment(HorizontalAlignment::Center)
            .build()
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;
    use ferrowl_ocpp::{V1_6, Version};

    const PROPS: &[PropSpec] = &[
        PropSpec {
            name: "connectorId",
            kind: PropKind::Number,
            source: PropSource::StateField("ConnectorId"),
            optional: false,
        },
        PropSpec {
            name: "idTag",
            kind: PropKind::Text,
            source: PropSource::Constant("ABC"),
            optional: false,
        },
        PropSpec {
            name: "note",
            kind: PropKind::Text,
            source: PropSource::Empty,
            optional: true,
        },
    ];

    fn spec() -> ActionSpec {
        ActionSpec {
            props: PROPS,
            assemble: flat_object,
            complex: false,
        }
    }

    fn dialog() -> ActionDialog {
        ActionDialog::new(
            "RemoteStartTransaction".into(),
            &spec(),
            |f| (f == "ConnectorId").then(|| "2".to_string()),
            || "tx-1".to_string(),
        )
    }

    #[test]
    fn assemble_coerces_kinds_and_prefills_state() {
        let d = dialog();
        // connectorId prefilled from state (number), idTag from constant, optional note omitted.
        assert_eq!(d.payload(), json!({ "connectorId": 2, "idTag": "ABC" }));
    }

    #[test]
    fn c_toggles_compact_in_table_mode_not_in_json() {
        let mut d = dialog();
        assert!(!d.compact);
        d.input(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(d.compact, "c toggles compact while the table is focused");
        // In JSON mode `c` is text input, not a compact toggle.
        d.toggle_mode();
        let before = d.compact;
        d.input(KeyModifiers::NONE, KeyCode::Char('c'));
        assert_eq!(d.compact, before, "c must not toggle compact in JSON mode");
    }

    #[test]
    fn set_charging_profile_charging_rate_unit_in_table() {
        let s = crate::module::ocpp::spec::v1_6::action_spec("SetChargingProfile").unwrap();
        let d = ActionDialog::new("SetChargingProfile".into(), &s, |_| None, || "t".into());
        assert_eq!(
            d.payload()["csChargingProfiles"]["chargingSchedule"]["chargingRateUnit"],
            "A"
        );
    }

    #[test]
    fn number_coercion_int_vs_float() {
        assert_eq!(ActionDialog::coerce(PropKind::Number, "3"), Some(json!(3)));
        assert_eq!(
            ActionDialog::coerce(PropKind::Number, "2.5"),
            Some(json!(2.5))
        );
        assert_eq!(ActionDialog::coerce(PropKind::Number, ""), None);
        assert_eq!(
            ActionDialog::coerce(PropKind::Bool, "true"),
            Some(json!(true))
        );
    }

    #[test]
    fn now_prefill_is_nonempty() {
        const TS: &[PropSpec] = &[PropSpec {
            name: "expiryDate",
            kind: PropKind::Timestamp,
            source: PropSource::Now,
            optional: false,
        }];
        let s = ActionSpec {
            props: TS,
            assemble: flat_object,
            complex: false,
        };
        let d = ActionDialog::new("X".into(), &s, |_| None, || "t".into());
        assert!(
            d.payload()["expiryDate"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
    }

    #[test]
    fn json_toggle_matches_assembled_rows() {
        let mut d = dialog();
        let assembled = d.payload();
        d.toggle_mode();
        assert!(d.json_mode);
        assert_eq!(d.payload(), assembled);
    }

    #[test]
    fn enter_on_row_opens_value_editor() {
        let mut d = dialog();
        assert!(d.editor.is_none());
        d.input(KeyModifiers::NONE, KeyCode::Enter); // focus Fields, table mode → open editor
        assert!(d.editor.is_some());
    }

    #[test]
    fn send_button_emits_decodable_payload() {
        // Drive a real spec end-to-end: assemble must decode into the typed action.
        let s = crate::module::ocpp::spec::v1_6::action_spec("ChangeAvailability").unwrap();
        let d = ActionDialog::new(
            "ChangeAvailability".into(),
            &s,
            |f| (f == "ConnectorId").then(|| "1".to_string()),
            || "t".into(),
        );
        let payload = d.payload();
        assert_eq!(payload["connectorId"], 1);
        assert_eq!(payload["type"], "Operative");
        assert!(V1_6::decode_call("ChangeAvailability", payload).is_ok());
    }

    #[test]
    fn nested_set_charging_profile_decodes() {
        let s = crate::module::ocpp::spec::v1_6::action_spec("SetChargingProfile").unwrap();
        let d = ActionDialog::new(
            "SetChargingProfile".into(),
            &s,
            |f| (f == "ConnectorId").then(|| "1".to_string()),
            || "t".into(),
        );
        let payload = d.payload();
        assert_eq!(payload["connectorId"], 1);
        assert_eq!(
            payload["csChargingProfiles"]["chargingSchedule"]["chargingSchedulePeriod"][0]["limit"],
            16
        );
        assert!(V1_6::decode_call("SetChargingProfile", payload).is_ok());
    }

    #[test]
    fn nested_send_local_list_single_entry_decodes() {
        let s = crate::module::ocpp::spec::v1_6::action_spec("SendLocalList").unwrap();
        let d = ActionDialog::new(
            "SendLocalList".into(),
            &s,
            |f| (f == "Rfid").then(|| "TAG1".to_string()),
            || "t".into(),
        );
        let payload = d.payload();
        assert_eq!(payload["localAuthorizationList"][0]["idTag"], "TAG1");
        assert!(V1_6::decode_call("SendLocalList", payload).is_ok());
    }

    #[test]
    fn nested_json_toggle_round_trips() {
        let s = crate::module::ocpp::spec::v1_6::action_spec("SetChargingProfile").unwrap();
        let mut d = ActionDialog::new("SetChargingProfile".into(), &s, |_| None, || "t".into());
        let assembled = d.payload();
        d.toggle_mode();
        assert!(d.json_mode);
        assert_eq!(d.payload(), assembled);
    }

    #[test]
    fn nested_notify_event_single_entry_decodes() {
        use ferrowl_ocpp::V2_0_1;
        let s = crate::module::ocpp::spec::v2_0_1::action_spec("NotifyEvent").unwrap();
        let d = ActionDialog::new("NotifyEvent".into(), &s, |_| None, || "t".into());
        let payload = d.payload();
        assert!(
            payload["eventData"][0]["component"]["name"]
                .as_str()
                .is_some()
        );
        assert!(V2_0_1::decode_call("NotifyEvent", payload).is_ok());
    }

    #[test]
    fn nested_set_charging_profile_201_decodes() {
        use ferrowl_ocpp::V2_0_1;
        let s = crate::module::ocpp::spec::v2_0_1::action_spec("SetChargingProfile").unwrap();
        let d = ActionDialog::new(
            "SetChargingProfile".into(),
            &s,
            |f| (f == "EvseId").then(|| "1".to_string()),
            || "t".into(),
        );
        let payload = d.payload();
        assert_eq!(
            payload["chargingProfile"]["chargingSchedule"][0]["chargingSchedulePeriod"][0]["limit"],
            16
        );
        assert!(V2_0_1::decode_call("SetChargingProfile", payload).is_ok());
    }
}
