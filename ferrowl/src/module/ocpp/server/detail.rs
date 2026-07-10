//! Per-entry detail overlay for the CSMS view, opened with Enter on a Charging Stations row.
//!
//! A CS-level overlay shows a read-only "State" table on top; below it the "Configuration" table
//! (fed from `GetConfiguration`/`GetVariables`, with a free-form "Fetch key" input) beside an
//! "RFIDs" accept-list table (with an "Add RFID" input). A connector overlay shows "State" above its
//! own "RFIDs" table/input on the left and a "Metering" table on the right; the connector RFID table
//! also lists the inherited CS tags read-only. The view feeds live rows in on each render and merges
//! config responses as they arrive; this struct owns the widgets, the accumulated config rows, the
//! RFID rows, and an optional value-input dialog.
//!
//! Keys: Esc closes (or closes the value dialog); Tab cycles focus; `c` (on a focused table)
//! toggles compact rows. In the Configuration table: `d` removes the selected key, `u` re-requests
//! its value, Enter opens a value dialog whose Enter writes the value (`ChangeConfiguration` /
//! `SetVariables`). Enter in the Fetch-key input requests that key. In the RFIDs table `d` removes
//! the selected own tag; Enter in the Add-RFID input adds a tag. Network actions and RFID edits are
//! returned to the view as a [`DetailRequest`].

use crossterm::event::{KeyCode, KeyModifiers};

use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmEvent};
use crate::module::ocpp::server::backend::Scope;
use crate::module::ocpp::widgets;
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{InputFieldState, TableState},
    style::InputFieldStyle,
    traits::HandleEvents,
    widgets::{GetValue, InputField, Table, Widget},
};
use ferrowl_ui_derive::TableEntry;
use ratatui::style::Style;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

/// A network action the overlay asks the view to perform on its target connection.
pub enum DetailRequest {
    /// Close the overlay.
    Close,
    /// Request the value of a configuration key (`GetConfiguration` / `GetVariables`).
    Fetch(String),
    /// Write a configuration value (`ChangeConfiguration` / `SetVariables`).
    Set(String, String),
    /// Add an RFID tag to this entry's accept-list (CS-level or connector, per the overlay scope).
    AddRfid(String),
    /// Remove an RFID tag from this entry's own accept-list.
    DelRfid(String),
}

// --- Key/value table -------------------------------------------------------

#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = KvHeader)]
struct KvRow {
    #[column(name = "Field", min = 16, max = 30)]
    key: String,
    #[column(name = "Unit", min = 6, max = 6)]
    unit: String,
    #[column(name = "Value", min = 10, max = 40)]
    value: String,
}

type KvTable = Widget<TableState<KvRow, 3>, Table<KvRow, KvHeader, 3>>;

// --- Configuration table (key/value/readonly) ------------------------------

#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = CfgHeader)]
struct CfgRow {
    #[column(name = "Key", min = 16, max = 30)]
    key: String,
    #[column(name = "Value", min = 10, max = 40)]
    value: String,
    #[column(name = "ReadOnly", min = 8, max = 9)]
    readonly: String,
}

type CfgTable = Widget<TableState<CfgRow, 3>, Table<CfgRow, CfgHeader, 3>>;

// --- Configuration table with a Component column (2.0.1 component/variable/value/readonly) -------

#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = CfgHeaderC)]
struct CfgRowC {
    #[column(name = "Component", min = 12, max = 24)]
    component: String,
    #[column(name = "Key", min = 12, max = 30)]
    key: String,
    #[column(name = "Value", min = 10, max = 40)]
    value: String,
    #[column(name = "ReadOnly", min = 8, max = 9)]
    readonly: String,
}

type CfgTableC = Widget<TableState<CfgRowC, 4>, Table<CfgRowC, CfgHeaderC, 4>>;

// --- RFID accept-list table (tag/source) -----------------------------------

#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = RfidHeader)]
struct RfidRow {
    #[column(name = "RFID", min = 12, max = 40)]
    tag: String,
    #[column(name = "Source", min = 6, max = 16)]
    source: String,
}

type RfidTable = Widget<TableState<RfidRow, 2>, Table<RfidRow, RfidHeader, 2>>;

/// Split a stored config key into `(component, variable)`. 2.0.1 keys are `Component/Variable`;
/// a key without a `/` has an empty component.
fn split_component(key: &str) -> (String, String) {
    match key.split_once('/') {
        Some((c, v)) => (c.to_string(), v.to_string()),
        None => (String::new(), key.to_string()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    State,
    Side,
    Key,
    /// The RFID accept-list table.
    Rfid,
    /// The "add RFID" input below the RFID table.
    RfidInput,
}

/// The detail overlay for one entry (CS-level or connector).
pub struct DetailOverlay {
    /// Charge-point identity of the target entry.
    pub identity: String,
    /// The target entry's scope (CS-level/connector/EVSE), used to re-find it each frame.
    pub scope: Scope,
    /// Whether this is a CS-level entry (config table + key input) or a connector (metering table).
    pub is_cs: bool,
    state: KvTable,
    /// Connector "Metering" table (connector entries only).
    side: KvTable,
    /// CS-level "Configuration" table (key/value/readonly), used when `component_col` is false.
    cfg: CfgTable,
    /// CS-level "Configuration" table with a Component column (2.0.1), used when `component_col`.
    cfg4: CfgTableC,
    /// Whether the config table carries a Component column (2.0.1) vs a flat key (1.6).
    component_col: bool,
    key_input: Widget<InputFieldState, InputField<String>>,
    /// Config rows (key, value, readonly) from `GetConfiguration`/`GetVariables` (CS-level only).
    config: Vec<(String, String, bool)>,
    /// RFID accept-list table: this entry's own tags plus, for a connector, the inherited CS tags.
    rfid: RfidTable,
    /// The "add RFID" input below the RFID table.
    rfid_input: Widget<InputFieldState, InputField<String>>,
    /// `(tag, inherited)` rows backing the RFID table; inherited rows are read-only (the CS tags
    /// shown for reference in a connector overlay).
    rfid_rows: Vec<(String, bool)>,
    focus: Focus,
    /// Compact (no vertical row margin) tables; toggled with `c`. Default off (margin 1).
    compact: bool,
    /// An open value-input dialog for the selected config key: (key, input widget).
    set_dialog: Option<(String, Widget<InputFieldState, InputField<String>>)>,
    /// Close-confirm popup, opened by Esc.
    close_confirm: Option<CloseConfirmDialog>,
}

impl DetailOverlay {
    pub fn new(identity: String, scope: Scope, component_col: bool) -> Self {
        let is_cs = !scope.is_connector();
        Self {
            identity,
            scope,
            is_cs,
            state: kv_table("State"),
            side: kv_table("Metering"),
            cfg: cfg_table(),
            cfg4: cfg4_table(),
            component_col,
            key_input: key_input(),
            config: Vec::new(),
            rfid: rfid_table(),
            rfid_input: rfid_input(),
            rfid_rows: Vec::new(),
            focus: Focus::State,
            compact: false,
            set_dialog: None,
            close_confirm: None,
        }
    }

    /// Replace the RFID table rows: this entry's `own` tags followed by `inherited` CS tags (the
    /// latter shown read-only in a connector overlay). Fed by the view each render; only rebuilds the
    /// widget when the data changes so the table selection stays put.
    pub fn set_rfids(&mut self, own: Vec<String>, inherited: Vec<String>) {
        let rows: Vec<(String, bool)> = own
            .into_iter()
            .map(|t| (t, false))
            .chain(inherited.into_iter().map(|t| (t, true)))
            .collect();
        if rows != self.rfid_rows {
            self.rfid_rows = rows;
            self.refresh_rfid_table();
        }
    }

    fn refresh_rfid_table(&mut self) {
        let is_cs = self.is_cs;
        let rows: Vec<RfidRow> = self
            .rfid_rows
            .iter()
            .map(|(tag, inherited)| RfidRow {
                tag: tag.clone(),
                source: if *inherited {
                    "CS (inherited)".to_string()
                } else if is_cs {
                    "CS".to_string()
                } else {
                    "connector".to_string()
                },
            })
            .collect();
        self.rfid.state.set_values(rows);
    }

    /// The selected RFID tag if it is a deletable (own, non-inherited) row.
    fn selected_own_rfid(&self) -> Option<String> {
        let i = self.rfid.state.table_state().selected()?;
        match self.rfid_rows.get(i) {
            Some((tag, false)) => Some(tag.clone()),
            _ => None,
        }
    }

    /// Replace the "State" table rows (the view supplies live `(field, unit, value)` each render).
    pub fn set_state_rows(&mut self, rows: Vec<(String, String, String)>) {
        self.state.state.set_values(to_kv(rows));
    }

    /// Replace the connector "Metering" table rows (no-op visual for CS-level entries).
    pub fn set_metering_rows(&mut self, rows: Vec<(String, String, String)>) {
        self.side.state.set_values(to_kv(rows));
    }

    /// Merge a fetched (key, value, readonly) config row, updating an existing key or appending it.
    pub fn merge_config(&mut self, key: String, value: String, readonly: bool) {
        if let Some(row) = self.config.iter_mut().find(|(k, _, _)| *k == key) {
            row.1 = value;
            row.2 = readonly;
        } else {
            self.config.push((key, value, readonly));
        }
        self.refresh_config_table();
    }

    fn refresh_config_table(&mut self) {
        let ro_label = |ro: bool| if ro { "yes" } else { "no" }.to_string();
        if self.component_col {
            let rows: Vec<CfgRowC> = self
                .config
                .iter()
                .map(|(k, v, ro)| {
                    let (component, key) = split_component(k);
                    CfgRowC {
                        component,
                        key,
                        value: v.clone(),
                        readonly: ro_label(*ro),
                    }
                })
                .collect();
            self.cfg4.state.set_values(rows);
        } else {
            let rows: Vec<CfgRow> = self
                .config
                .iter()
                .map(|(k, v, ro)| CfgRow {
                    key: k.clone(),
                    value: v.clone(),
                    readonly: ro_label(*ro),
                })
                .collect();
            self.cfg.state.set_values(rows);
        }
    }

    /// Seed the config table from persisted rows (on overlay open).
    pub fn set_config(&mut self, rows: Vec<(String, String, bool)>) {
        self.config = rows;
        self.refresh_config_table();
    }

    /// The current config rows (to persist on overlay close).
    pub fn config_rows(&self) -> Vec<(String, String, bool)> {
        self.config.clone()
    }

    /// The free-form key currently typed (CS-level fetch target).
    pub fn key(&self) -> String {
        self.key_input.state.get_value()
    }

    /// Whether the key input is focused.
    pub fn key_focused(&self) -> bool {
        self.is_cs && self.focus == Focus::Key
    }

    /// Focus order for Tab cycling. CS: State → Configuration → Fetch-key → RFIDs → Add-RFID.
    /// Connector: State → Metering → RFIDs → Add-RFID.
    fn focus_order(&self) -> &'static [Focus] {
        if self.is_cs {
            &[
                Focus::State,
                Focus::Side,
                Focus::Key,
                Focus::Rfid,
                Focus::RfidInput,
            ]
        } else {
            &[Focus::State, Focus::Side, Focus::Rfid, Focus::RfidInput]
        }
    }

    pub fn focus_next(&mut self) {
        let order = self.focus_order();
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = order[(i + 1) % order.len()];
    }

    pub fn focus_previous(&mut self) {
        let order = self.focus_order();
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = order[(i + order.len() - 1) % order.len()];
    }

    fn toggle_compact(&mut self) {
        self.compact = !self.compact;
        let margin = Margin {
            vertical: if self.compact { 0 } else { 1 },
            horizontal: 0,
        };
        self.state.widget.set_row_margin(margin);
        self.side.widget.set_row_margin(margin);
        self.cfg.widget.set_row_margin(margin);
        self.cfg4.widget.set_row_margin(margin);
        self.rfid.widget.set_row_margin(margin);
    }

    /// The selected row index of the active config table.
    fn config_selected(&self) -> Option<usize> {
        if self.component_col {
            self.cfg4.state.table_state().selected()
        } else {
            self.cfg.state.table_state().selected()
        }
    }

    /// The selected configuration key, if the config table has a selection.
    fn selected_config_key(&self) -> Option<String> {
        let i = self.config_selected()?;
        self.config.get(i).map(|(k, _, _)| k.clone())
    }

    fn delete_selected_config(&mut self) {
        if let Some(i) = self.config_selected()
            && i < self.config.len()
        {
            self.config.remove(i);
            self.refresh_config_table();
        }
    }

    fn open_set_dialog(&mut self, key: String) {
        let current = self
            .config
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, v, _)| v.clone())
            .unwrap_or_default();
        let mut field = value_input(&key);
        field.state.set_input(current.clone());
        field.state.set_cursor(current.chars().count());
        self.set_dialog = Some((key, field));
    }

    /// Handle a key event. Returns a [`DetailRequest`] when the view must act on the connection.
    pub fn input(&mut self, modifiers: KeyModifiers, code: KeyCode) -> Option<DetailRequest> {
        // The value-input dialog captures all keys while open.
        if let Some((key, field)) = self.set_dialog.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.set_dialog = None,
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    let (k, v) = (key.clone(), field.state.get_value());
                    self.set_dialog = None;
                    // Optimistically reflect the new value; a freshly written key is writable.
                    self.merge_config(k.clone(), v.clone(), false);
                    return Some(DetailRequest::Set(k, v));
                }
                _ => {
                    let _ = field.state.handle_events(modifiers, code);
                }
            }
            return None;
        }

        // The close-confirm popup captures all keys while open.
        if let Some(confirm) = self.close_confirm.as_mut() {
            return match confirm.handle_key(modifiers, code) {
                CloseConfirmEvent::Close => {
                    self.close_confirm = None;
                    Some(DetailRequest::Close)
                }
                CloseConfirmEvent::Dismiss => {
                    self.close_confirm = None;
                    None
                }
                CloseConfirmEvent::Consumed => None,
            };
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.close_confirm = Some(CloseConfirmDialog::new());
            }
            (KeyModifiers::NONE, KeyCode::Tab) => self.focus_next(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => self.focus_previous(),
            // `c` toggles compact rows when a table is focused (the inputs keep `c` as text).
            (KeyModifiers::NONE, KeyCode::Char('c'))
                if !matches!(self.focus, Focus::Key | Focus::RfidInput) =>
            {
                self.toggle_compact()
            }
            // `d` on the RFID table removes the selected own (non-inherited) tag.
            (KeyModifiers::NONE, KeyCode::Char('d')) if self.focus == Focus::Rfid => {
                if let Some(tag) = self.selected_own_rfid() {
                    return Some(DetailRequest::DelRfid(tag));
                }
            }
            // Enter in the add-RFID input adds the typed tag (cleared for the next entry).
            (KeyModifiers::NONE, KeyCode::Enter) if self.focus == Focus::RfidInput => {
                let tag = self.rfid_input.state.get_value().trim().to_string();
                self.rfid_input.state.set_input(String::new());
                self.rfid_input.state.set_cursor(0);
                if !tag.is_empty() {
                    return Some(DetailRequest::AddRfid(tag));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) if self.is_cs && self.focus == Focus::Side => {
                self.delete_selected_config()
            }
            (KeyModifiers::NONE, KeyCode::Char('u')) if self.is_cs && self.focus == Focus::Side => {
                if let Some(key) = self.selected_config_key() {
                    return Some(DetailRequest::Fetch(key));
                }
            }
            (KeyModifiers::NONE, KeyCode::Enter) if self.is_cs && self.focus == Focus::Side => {
                if let Some(key) = self.selected_config_key() {
                    self.open_set_dialog(key);
                }
            }
            (KeyModifiers::NONE, KeyCode::Enter) if self.key_focused() => {
                let key = self.key();
                // Clear the input so the next key can be typed straight away.
                self.key_input.state.set_input(String::new());
                self.key_input.state.set_cursor(0);
                return Some(DetailRequest::Fetch(key));
            }
            _ => self.route(modifiers, code),
        }
        None
    }

    /// Route a key event to the focused pane (table scroll or key-input editing).
    fn route(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        match self.focus {
            Focus::State => {
                let _ = self.state.state.handle_events(modifiers, code);
            }
            Focus::Side if self.is_cs && self.component_col => {
                let _ = self.cfg4.state.handle_events(modifiers, code);
            }
            Focus::Side if self.is_cs => {
                let _ = self.cfg.state.handle_events(modifiers, code);
            }
            Focus::Side => {
                let _ = self.side.state.handle_events(modifiers, code);
            }
            Focus::Key => {
                let _ = self.key_input.state.handle_events(modifiers, code);
            }
            Focus::Rfid => {
                let _ = self.rfid.state.handle_events(modifiers, code);
            }
            Focus::RfidInput => {
                let _ = self.rfid_input.state.handle_events(modifiers, code);
            }
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let [_, hc, _] = Layout::horizontal([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(area);
        let [_, vc, _] = Layout::vertical([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .areas(hc);

        UiWidget::render(&Clear, vc, buf);
        let title = if self.scope.is_connector() {
            format!("{} — connector {}", self.identity, self.scope.label())
        } else {
            self.identity.clone()
        };
        let block = Block::bordered()
            .style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg))
            .title_alignment(HorizontalAlignment::Center)
            .title(title);
        let inner = block.inner(vc);
        block.render(vc, buf);
        let inner = inner.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        self.state.state.set_focused(self.focus == Focus::State);
        self.side.state.set_focused(self.focus == Focus::Side);
        self.cfg.state.set_focused(self.focus == Focus::Side);
        self.cfg4.state.set_focused(self.focus == Focus::Side);
        self.rfid.state.set_focused(self.focus == Focus::Rfid);
        self.rfid_input
            .state
            .set_focused(self.focus == Focus::RfidInput);

        if self.is_cs {
            // State on top; below it the Configuration (+ fetch input) beside the RFID list
            // (+ add input).
            let [state_area, mid] =
                Layout::vertical([Constraint::Percentage(35), Constraint::Min(1)]).areas(inner);
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(mid);
            let [config_area, fetch_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(left);
            let [rfid_area, rfid_input_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(right);
            self.key_input.state.set_focused(self.focus == Focus::Key);
            StatefulWidget::render(&self.state.widget, state_area, buf, &mut self.state.state);
            if self.component_col {
                StatefulWidget::render(&self.cfg4.widget, config_area, buf, &mut self.cfg4.state);
            } else {
                StatefulWidget::render(&self.cfg.widget, config_area, buf, &mut self.cfg.state);
            }
            StatefulWidget::render(
                &self.key_input.widget,
                fetch_area,
                buf,
                &mut self.key_input.state,
            );
            StatefulWidget::render(&self.rfid.widget, rfid_area, buf, &mut self.rfid.state);
            StatefulWidget::render(
                &self.rfid_input.widget,
                rfid_input_area,
                buf,
                &mut self.rfid_input.state,
            );
        } else {
            // Connector: State above the RFID list (+ add input) on the left, Metering on the right.
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Min(1)]).areas(inner);
            let [state_area, rfid_area, rfid_input_area] = Layout::vertical([
                Constraint::Percentage(45),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .areas(left);
            StatefulWidget::render(&self.state.widget, state_area, buf, &mut self.state.state);
            StatefulWidget::render(&self.rfid.widget, rfid_area, buf, &mut self.rfid.state);
            StatefulWidget::render(
                &self.rfid_input.widget,
                rfid_input_area,
                buf,
                &mut self.rfid_input.state,
            );
            StatefulWidget::render(&self.side.widget, right, buf, &mut self.side.state);
        }

        if let Some((_key, field)) = self.set_dialog.as_mut() {
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
            field.state.set_focused(true);
            StatefulWidget::render(&field.widget, mid, buf, &mut field.state);
        }

        if let Some(confirm) = self.close_confirm.as_mut() {
            confirm.render(vc, buf);
        }
    }
}

fn to_kv(rows: Vec<(String, String, String)>) -> Vec<KvRow> {
    rows.into_iter()
        .map(|(key, unit, value)| KvRow { key, unit, value })
        .collect()
}

fn kv_table(title: &str) -> KvTable {
    widgets::table(title)
}

fn cfg_table() -> CfgTable {
    widgets::table("Configuration")
}

fn cfg4_table() -> CfgTableC {
    widgets::table("Configuration")
}

fn rfid_table() -> RfidTable {
    widgets::table("RFIDs")
}

fn rfid_input() -> Widget<InputFieldState, InputField<String>> {
    widgets::input(
        ("Add RFID", HorizontalAlignment::Left),
        "rfid tag (Enter to add)",
        false,
        widgets::bordered_input_style(),
    )
}

fn key_input() -> Widget<InputFieldState, InputField<String>> {
    widgets::input(
        ("Fetch key", HorizontalAlignment::Left),
        "config key (Enter to fetch)",
        false,
        widgets::bordered_input_style(),
    )
}

fn value_input(key: &str) -> Widget<InputFieldState, InputField<String>> {
    widgets::input(
        (format!("Set {key}"), HorizontalAlignment::Left),
        "new value (Enter to set)",
        true,
        InputFieldStyle::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_config_updates_existing_and_appends_new() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.merge_config("A".into(), "1".into(), false);
        d.merge_config("B".into(), "2".into(), false);
        d.merge_config("A".into(), "9".into(), false); // updates A in place
        assert_eq!(
            d.config,
            vec![
                ("A".into(), "9".into(), false),
                ("B".into(), "2".into(), false)
            ]
        );
    }

    #[test]
    fn connector_overlay_has_no_key_pane() {
        let mut d = DetailOverlay::new("CP".into(), Scope::connector(1), false);
        assert!(!d.is_cs);
        // Focus cycles State -> Side -> State; the key input pane is CS-only.
        d.focus_next();
        assert!(!d.key_focused());
        d.focus_next();
        assert!(!d.key_focused());
    }

    #[test]
    fn cs_overlay_reaches_key_pane() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        assert!(d.is_cs);
        // State -> Side -> Key.
        d.focus_next();
        d.focus_next();
        assert!(d.key_focused());
    }

    #[test]
    fn toggle_compact_flips_flag() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        assert!(!d.compact);
        // `c` on a focused table toggles; on the key input it is text.
        assert!(d.input(KeyModifiers::NONE, KeyCode::Char('c')).is_none());
        assert!(d.compact);
        d.focus = Focus::Key;
        d.input(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(
            d.compact,
            "compact must not toggle while typing in the key input"
        );
    }

    #[test]
    fn component_col_splits_key_into_component_and_variable() {
        // 2.0.1 overlay: a `Component/Variable` key is split across the Component + Key columns.
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, true);
        d.merge_config("OCPPCommCtrlr/HeartbeatInterval".into(), "30".into(), true);
        // A key without a `/` keeps an empty component.
        d.merge_config("Bare".into(), "x".into(), false);
        let rows = d.cfg4.state.values();
        assert_eq!(rows[0].component, "OCPPCommCtrlr");
        assert_eq!(rows[0].key, "HeartbeatInterval");
        assert_eq!(rows[0].readonly, "yes");
        assert_eq!(rows[1].component, "");
        assert_eq!(rows[1].key, "Bare");
        // The selected key fed to fetch/set is the full combined `Component/Variable`.
        d.focus = Focus::Side;
        let req = d.input(KeyModifiers::NONE, KeyCode::Char('u'));
        assert!(
            matches!(req, Some(DetailRequest::Fetch(k)) if k == "OCPPCommCtrlr/HeartbeatInterval")
        );
    }

    #[test]
    fn rfid_input_enter_emits_add_request() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.focus = Focus::RfidInput;
        d.rfid_input.state.set_input("DEADBEEF".to_string());
        let req = d.input(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(req, Some(DetailRequest::AddRfid(t)) if t == "DEADBEEF"));
        // The input is cleared for the next tag.
        assert_eq!(d.rfid_input.state.get_value(), "");
    }

    #[test]
    fn rfid_delete_targets_own_rows_only() {
        let mut d = DetailOverlay::new("CP".into(), Scope::connector(1), false);
        // Own tag first, then the inherited CS tag (read-only).
        d.set_rfids(vec!["OWN".into()], vec!["CS".into()]);
        let rows = d.rfid.state.values();
        assert_eq!(rows[0].source, "connector");
        assert_eq!(rows[1].source, "CS (inherited)");
        d.focus = Focus::Rfid;
        // Row 0 (own) is deletable.
        let req = d.input(KeyModifiers::NONE, KeyCode::Char('d'));
        assert!(matches!(req, Some(DetailRequest::DelRfid(t)) if t == "OWN"));
        // Row 1 (inherited) is not.
        d.input(KeyModifiers::NONE, KeyCode::Down);
        assert!(d.input(KeyModifiers::NONE, KeyCode::Char('d')).is_none());
    }

    #[test]
    fn focus_cycle_includes_rfid_panes() {
        // CS: State -> Side -> Key -> Rfid -> RfidInput -> wrap.
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        for expected in [
            Focus::Side,
            Focus::Key,
            Focus::Rfid,
            Focus::RfidInput,
            Focus::State,
        ] {
            d.focus_next();
            assert!(d.focus == expected);
        }
        // Connector: State -> Side -> Rfid -> RfidInput -> wrap (no Key pane).
        let mut c = DetailOverlay::new("CP".into(), Scope::connector(1), false);
        for expected in [Focus::Side, Focus::Rfid, Focus::RfidInput, Focus::State] {
            c.focus_next();
            assert!(c.focus == expected);
        }
    }

    #[test]
    fn config_table_shows_readonly_column() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.merge_config("A".into(), "1".into(), true);
        d.merge_config("B".into(), "2".into(), false);
        let rows = d.cfg.state.values();
        assert_eq!(rows[0].readonly, "yes");
        assert_eq!(rows[1].readonly, "no");
        // A value written via the set dialog is marked writable (not read-only).
        d.focus = Focus::Side;
        d.input(KeyModifiers::NONE, KeyCode::Enter); // open set dialog for row 0 (A)
        d.input(KeyModifiers::NONE, KeyCode::Enter); // confirm (keeps prefilled value)
        assert_eq!(d.config[0], ("A".into(), "1".into(), false));
    }

    #[test]
    fn delete_removes_selected_config_key() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.merge_config("A".into(), "1".into(), false);
        d.merge_config("B".into(), "2".into(), false);
        d.focus = Focus::Side;
        // Selection defaults to row 0 (A).
        d.input(KeyModifiers::NONE, KeyCode::Char('d'));
        assert_eq!(d.config, vec![("B".into(), "2".into(), false)]);
    }

    #[test]
    fn refetch_selected_config_key_requests_fetch() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.merge_config("HeartbeatInterval".into(), "30".into(), false);
        d.focus = Focus::Side;
        let req = d.input(KeyModifiers::NONE, KeyCode::Char('u'));
        assert!(matches!(req, Some(DetailRequest::Fetch(k)) if k == "HeartbeatInterval"));
    }

    #[test]
    fn enter_on_config_opens_value_dialog_then_set_request() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.merge_config("HeartbeatInterval".into(), "30".into(), false);
        d.focus = Focus::Side;
        // Enter opens the value dialog (no request yet).
        assert!(d.input(KeyModifiers::NONE, KeyCode::Enter).is_none());
        assert!(d.set_dialog.is_some());
        // Edit the value then confirm.
        d.input(KeyModifiers::CONTROL, KeyCode::Char('d')); // clear prefilled "30"
        for c in "45".chars() {
            d.input(KeyModifiers::NONE, KeyCode::Char(c));
        }
        let req = d.input(KeyModifiers::NONE, KeyCode::Enter);
        assert!(
            matches!(req, Some(DetailRequest::Set(k, v)) if k == "HeartbeatInterval" && v == "45")
        );
        assert!(d.set_dialog.is_none());
    }

    #[test]
    fn fetch_clears_key_input() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.focus = Focus::Key;
        for c in "Foo".chars() {
            d.input(KeyModifiers::NONE, KeyCode::Char(c));
        }
        let req = d.input(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(req, Some(DetailRequest::Fetch(k)) if k == "Foo"));
        assert_eq!(d.key(), "", "key input must be cleared after a fetch");
    }

    #[test]
    fn esc_returns_none() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        assert!(d.input(KeyModifiers::NONE, KeyCode::Esc).is_none());
        assert!(d.close_confirm.is_some());
        assert!(d.input(KeyModifiers::NONE, KeyCode::Esc).is_none());
        assert!(d.close_confirm.is_none());
    }

    #[test]
    fn esc_then_enter_returns_close() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        assert!(d.input(KeyModifiers::NONE, KeyCode::Esc).is_none());
        assert!(d.close_confirm.is_some());
        assert!(matches!(
            d.input(KeyModifiers::NONE, KeyCode::Enter),
            Some(DetailRequest::Close)
        ));
    }

    #[test]
    fn set_value_dialog_esc_cancels() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.merge_config("HeartbeatInterval".into(), "30".into(), false);
        d.focus = Focus::Side;
        assert!(d.input(KeyModifiers::NONE, KeyCode::Enter).is_none());
        assert!(d.set_dialog.is_some());
        assert!(d.input(KeyModifiers::NONE, KeyCode::Esc).is_none());
        assert!(d.set_dialog.is_none());
    }

    #[test]
    fn colon_in_rfid_input_types() {
        let mut d = DetailOverlay::new("CP".into(), Scope::CS, false);
        d.focus = Focus::RfidInput;
        d.input(KeyModifiers::NONE, KeyCode::Char(':'));
        assert!(d.close_confirm.is_none());
        assert_eq!(d.rfid_input.state.get_value(), ":");
    }
}
