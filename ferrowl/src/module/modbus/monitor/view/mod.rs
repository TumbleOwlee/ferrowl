//! `ModbusMonitorModuleView` (UI-R-060/061): the monitor module's content view — a left panel
//! listing observed unit ids, and three right-hand sections scoped to the selected unit id: a
//! message table (MB-R-143), a memory layout grouped by table kind (MB-R-144), and a
//! resolved-registers table (MB-R-145), the last hidden entirely when no interpretation exists
//! for the selected unit id.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_codec::Kind;
use ferrowl_modbus::monitor::{MonitorRecord, RecordStatus};
use ferrowl_modbus::{Key, SlaveKey, UnitId};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::{HandleEvents, SetFocus};
use ferrowl_ui::{
    Border,
    state::{TableState, TableStateBuilder},
    style::TableStyleBuilder,
    widgets::{Header, Table, TableBuilder, Widget},
};
use ferrowl_ui_derive::TableEntry;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;

use crate::app::Level;
use crate::config::device::MonitorRegisterDef;
use crate::config::{ModuleSpec, MonitorDeviceConfig};
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, CommandSpec, ModuleView, RefreshFuture,
    SharedLog, parse_command,
};

use super::ModbusMonitorModule;
use super::dialog::{AddInterpretationDialog, EditInterpretationDialog};
use super::setup_dialog::MonitorSetupDialog;

/// Which of the view's Tab-cyclable panels currently has focus (default `Units`), matching
/// the OCPP module view's per-panel border-highlight pattern. The resolved-registers panel is
/// display-only and not part of the cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MonitorPanel {
    #[default]
    Units,
    Messages,
    Memory,
    /// UI-R-065 — only reachable in the Tab cycle while the selected unit id has at least one
    /// interpretation (`handle_events`' Tab/BackTab handling skips it otherwise, since `next`/
    /// `previous` here have no access to that dynamic state).
    Resolved,
}

impl MonitorPanel {
    fn next(self) -> Self {
        match self {
            MonitorPanel::Units => MonitorPanel::Messages,
            MonitorPanel::Messages => MonitorPanel::Memory,
            MonitorPanel::Memory => MonitorPanel::Resolved,
            MonitorPanel::Resolved => MonitorPanel::Units,
        }
    }

    fn previous(self) -> Self {
        match self {
            MonitorPanel::Units => MonitorPanel::Resolved,
            MonitorPanel::Messages => MonitorPanel::Units,
            MonitorPanel::Memory => MonitorPanel::Messages,
            MonitorPanel::Resolved => MonitorPanel::Memory,
        }
    }
}

/// The single modal overlay over the monitor view (mutually exclusive by construction).
enum MonitorOverlay {
    None,
    /// `:add`/`:a` interpretation dialog (UI-R-061), scoped to the currently selected unit id.
    Add(Box<AddInterpretationDialog>),
    /// `:edit`/`:e` re-setup dialog, prefilled from the current spec/device. Renamed from
    /// `Edit` (mechanical rename, `s15`) to make room for `EditInterpretation` below, which
    /// this could otherwise be confused with.
    EditSetup(Box<MonitorSetupDialog>),
    /// MB-R-148 — `Enter` on a Resolved-registers row (`s14`) opens this, prefilled from the
    /// selected row. The `String` is the interpretation's original name, needed for the
    /// edit-in-place lookup (`module.edit_interpretation`'s `old_name` — a confirmed rename
    /// changes the map key, so the dialog's own current label input isn't enough once the user
    /// edits it).
    EditInterpretation(Box<EditInterpretationDialog>, String),
}

/// Save `device` to `path`, mirroring `ModbusModuleView::save_device_to`'s pattern (stamps the
/// current `VERSION`, format from the path's extension).
#[allow(dead_code)] // consumed starting s8
fn save_device_to(device: &MonitorDeviceConfig, path: &str) -> CommandResult {
    use ferrowl_util::convert::{Converter, FileType};
    let Some(ty) = FileType::from_path(path) else {
        return CommandResult::Handled(Some((
            Level::Warning,
            format!("unknown format for '{path}' (use .toml or .json)"),
        )));
    };
    let mut device = device.clone();
    device.version = Some(crate::config::VERSION.to_string());
    match Converter::save(&device, path, ty) {
        Ok(()) => CommandResult::Handled(Some((
            Level::Info,
            format!("Saved device config to {path}"),
        ))),
        Err(e) => CommandResult::Handled(Some((Level::Error, format!("Save failed: {e:?}")))),
    }
}

/// The 4 register-table kinds a monitor's observed-value table groups its memory layout by
/// (MB-R-144), in display order.
#[allow(dead_code)] // consumed starting s8
const TABLE_KINDS: [Kind; 4] = [
    Kind::Coil,
    Kind::DiscreteInput,
    Kind::HoldingRegister,
    Kind::InputRegister,
];

/// UI-R-062 — one row of the Messages table: a `MonitorRecord`, sourced from `module.records()`,
/// rendered most-recent-first in this fixed column order.
#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = MessageHeader)]
struct MessageRow {
    #[column(name = "Time", min = 12, max = 12)]
    time: String,
    #[column(name = "Status", min = 10, max = 16)]
    status: String,
    #[column(name = "Slave", min = 6, max = 6)]
    slave: String,
    #[column(name = "Operation", min = 14, max = 30)]
    operation: String,
    #[column(name = "Address", min = 8, max = 16)]
    address: String,
    #[column(name = "Quantity", min = 8, max = 16)]
    quantity: String,
    #[column(name = "Values/Payload", min = 10, max = 800)]
    payload: String,
}

type MessageTable = Widget<TableState<MessageRow, 7>, Table<MessageRow, MessageHeader, 7>>;

/// UI-R-062 — build a fresh, empty `MessageTable` widget/state pair, same builder shape
/// `ferrowl/src/module/modbus/table.rs`'s `TableView::new` already uses.
fn new_message_table() -> MessageTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
            .title(Some("Messages".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .build()
            .expect("all required builder fields are set"),
    }
}

/// UI-R-062's `Status` column text.
fn format_record_status(status: &RecordStatus) -> String {
    match status {
        RecordStatus::Ok => "OK".to_string(),
        RecordStatus::Unmatched => "Unmatched".to_string(),
        RecordStatus::Exception(code) => format!("Exception({code:?})"),
    }
}

/// Shared hex-words formatting (UI-R-062's Messages payload, UI-R-064's resolved-registers raw
/// value): 4-digit lowercase hex per word, bracket-wrapped, space-separated.
fn hex_words(words: &[u16]) -> String {
    format!(
        "[{}]",
        words
            .iter()
            .map(|v| format!("{v:04x}"))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// UI-R-062's `Values/Payload` column text — empty for no shape or no values (MB-R-146's
/// record-status-to-value gating, Shared); one digit per bit for coil-family kinds, 4-digit
/// lowercase hex per word for register-family kinds.
fn format_record_payload(record: &MonitorRecord) -> String {
    let Some(shape) = &record.shape else {
        return String::new();
    };
    if shape.values.is_empty() {
        return String::new();
    }
    match shape.kind {
        Kind::Coil | Kind::DiscreteInput => format!(
            "[{}]",
            shape
                .values
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Kind::HoldingRegister | Kind::InputRegister => hex_words(&shape.values),
    }
}

/// UI-R-062's `Address`/`Quantity` columns — empty for no shape; `edge-cases.md`'s
/// `ReadWriteMultipleRegisters` row (both a read and a write address/quantity pair) renders
/// slash-separated, otherwise the single address/quantity.
fn format_record_address_quantity(record: &MonitorRecord) -> (String, String) {
    let Some(shape) = &record.shape else {
        return (String::new(), String::new());
    };
    match (shape.write_address, shape.write_quantity) {
        (Some(write_address), Some(write_quantity)) => (
            format!("{}/{}", shape.address, write_address),
            format!("{}/{}", shape.quantity, write_quantity),
        ),
        _ => (shape.address.to_string(), shape.quantity.to_string()),
    }
}

/// UI-R-062 — build one `MessageRow` for `unit`'s Messages table from a captured record.
/// `time` has no wall-clock reference available from `MonitorRecord.timestamp` (an `Instant`),
/// so it renders elapsed-seconds-ago relative to `now`.
fn message_row(unit: UnitId, record: &MonitorRecord, now: std::time::Instant) -> MessageRow {
    let (address, quantity) = format_record_address_quantity(record);
    MessageRow {
        time: format!("{}s ago", now.duration_since(record.timestamp).as_secs()),
        status: format_record_status(&record.status),
        slave: unit.to_string(),
        operation: format!("{:?}", record.operation),
        address,
        quantity,
        payload: format_record_payload(record),
    }
}

/// UI-R-064 — one row of the Resolved-registers table: the same column set as
/// `ferrowl::module::modbus::table::TableHeader` minus Slave ID/Access (Shared).
#[derive(Clone, Debug, Default, TableEntry)]
#[table_entry(header = ResolvedHeader)]
struct ResolvedRow {
    #[column(name = "Name", min = 15, max = 30)]
    name: String,
    #[column(name = "Description", min = 25, max = 40)]
    description: String,
    #[column(name = "Address", min = 10, max = 20)]
    address: String,
    #[column(name = "Kind", min = 20, max = 30)]
    kind: String,
    #[column(name = "Format", min = 10, max = 20)]
    format: String,
    #[column(name = "Length", min = 10, max = 20)]
    length: String,
    #[column(name = "Resolution", min = 10, max = 20)]
    resolution: String,
    #[column(name = "Value", min = 5, max = 40)]
    value: String,
    #[column(name = "Raw Value", min = 0, max = 800)]
    raw_value: String,
}

type ResolvedTable = Widget<TableState<ResolvedRow, 9>, Table<ResolvedRow, ResolvedHeader, 9>>;

/// UI-R-064 — build a fresh, empty `ResolvedTable` widget/state pair, same builder shape as
/// `new_message_table` (Shared).
fn new_resolved_table() -> ResolvedTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
            .title(Some("Resolved registers".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            // Expanded (non-compact) is the default (tui/api-contract.md §2.1): 1 row of
            // vertical padding, same as `TableView::new`'s own default (Shared).
            .row_margin(ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

/// UI-R-064 — build one `ResolvedRow` for `name`/`def`, reading `unit`'s currently observed
/// words from `table`. `value`/`raw_value` render `"(not yet observed)"`/empty when nothing has
/// been observed at `def`'s address yet.
fn resolved_row(
    name: &str,
    def: &MonitorRegisterDef,
    table: &ferrowl_modbus::monitor::ObservedTable,
    unit: UnitId,
) -> ResolvedRow {
    let key = Key::new(SlaveKey {
        slave_id: unit,
        kind: def.kind.clone(),
    });
    let address = match def.address() {
        ferrowl_codec::Address::Fixed(a) => a,
        ferrowl_codec::Address::Virtual => 0,
    };
    let width = def.format().width();
    let (value, raw_value) = match table.read_words(&key, address, width) {
        Some(words) => (format!("{words:?}"), hex_words(&words)),
        None => ("(not yet observed)".to_string(), String::new()),
    };
    let resolution = match def.format().resolution() {
        Some(v) => format!("{v}"),
        None => "None".to_string(),
    };
    ResolvedRow {
        name: name.to_string(),
        description: def.description.clone(),
        address: address.to_string(),
        kind: format!("{:?}", def.kind),
        format: format!("{}", def.format()),
        length: format!("{width}"),
        resolution,
        value,
        raw_value,
    }
}

/// UI-R-063 — one hex-editor cell of the Memory-layout panel: an observed-or-not raw value at
/// one raw address (coil-family: one packed byte covering 8 bit addresses; register-family: one
/// word at one word address).
#[derive(Debug, Clone, Copy, PartialEq)]
struct MemoryCell {
    observed: bool,
    value: u16,
}

/// UI-R-063 — `(raw addresses per cell, cells per rendered line)` for `kind`: coil-family packs
/// 8 bits per byte-cell, 16 bytes (128 bit addresses) per line; register-family is 1 word per
/// cell, 8 words per line.
fn memory_cell_shape(kind: Kind) -> (u16, u16) {
    match kind {
        Kind::Coil | Kind::DiscreteInput => (8, 16),
        Kind::HoldingRegister | Kind::InputRegister => (1, 8),
    }
}

/// UI-R-063 — group `pairs` (from [`ModbusMonitorModuleView::memory_rows`], already
/// address-ordered `(address, value)` pairs) into fixed-width hex-editor lines per
/// `memory_cell_shape`, MSB-first bit-packed for coil-family kinds. Only lines containing at
/// least one observed cell are returned, sorted by starting address. A cell within an otherwise
/// -observed line that itself has no observed constituent bit/word renders unobserved
/// (`MemoryCell::observed == false`, `value == 0`) — for coil-family kinds this is a documented
/// sub-byte-granularity call: a byte with only some of its 8 bits observed still counts as an
/// observed cell, with its unobserved bits packed as `0`.
fn memory_lines(kind: Kind, pairs: &[(u16, u16)]) -> Vec<(u16, Vec<MemoryCell>)> {
    let (unit_per_cell, cells_per_line) = memory_cell_shape(kind);
    let observed: std::collections::HashMap<u16, u16> = pairs.iter().copied().collect();

    let mut line_starts: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for &(address, _) in pairs {
        let cell_addr = address / unit_per_cell;
        let line_start_cell = (cell_addr / cells_per_line) * cells_per_line;
        line_starts.insert(line_start_cell * unit_per_cell);
    }

    line_starts
        .into_iter()
        .map(|line_start_address| {
            let line_start_cell = line_start_address / unit_per_cell;
            let cells = (0..cells_per_line)
                .map(|i| {
                    let cell_addr = line_start_cell + i;
                    if unit_per_cell == 8 {
                        let mut value: u16 = 0;
                        let mut cell_observed = false;
                        for bit in 0..8u16 {
                            let raw = cell_addr * 8 + bit;
                            if let Some(&bit_value) = observed.get(&raw) {
                                cell_observed = true;
                                if bit_value != 0 {
                                    value |= 1 << (7 - bit);
                                }
                            }
                        }
                        MemoryCell {
                            observed: cell_observed,
                            value,
                        }
                    } else {
                        match observed.get(&cell_addr) {
                            Some(&v) => MemoryCell {
                                observed: true,
                                value: v,
                            },
                            None => MemoryCell {
                                observed: false,
                                value: 0,
                            },
                        }
                    }
                })
                .collect();
            (line_start_address, cells)
        })
        .collect()
}

/// Whether `value`'s low byte is a printable ASCII character (space or graphic) — the same
/// printable-vs-`.` convention `Definition::values()` uses (`ferrowl/src/module/modbus/table.rs`).
fn is_printable_low_byte(value: u16) -> bool {
    let byte = (value & 0xFF) as u8;
    byte == b' ' || byte.is_ascii_graphic()
}

/// UI-R-063's value-class color for one cell: unobserved or an observed zero reads as neutral
/// (`placeholder`); an observed printable-ASCII low byte as normal text; any other observed
/// non-zero value flagged (`warning`).
fn memory_cell_value_style(cell: &MemoryCell) -> ratatui::style::Color {
    use ferrowl_ui::COLOR_SCHEME;
    if !cell.observed || cell.value == 0 {
        COLOR_SCHEME.placeholder
    } else if is_printable_low_byte(cell.value) {
        COLOR_SCHEME.text
    } else {
        COLOR_SCHEME.warning
    }
}

/// The character-representation column for one cell: `.` when unobserved or non-printable, else
/// the printable low-byte character (`Definition::values()`'s convention).
fn memory_cell_char(cell: &MemoryCell) -> char {
    if cell.observed && is_printable_low_byte(cell.value) {
        (cell.value & 0xFF) as u8 as char
    } else {
        '.'
    }
}

/// UI-R-063 — one row of the Memory-layout hex-editor render: either a table-kind line (its
/// starting address + cells) or a blank `gap_bg` separator between two non-adjacent lines.
enum MemoryLine {
    Line {
        kind: Kind,
        address: u16,
        cells: Vec<MemoryCell>,
    },
    Gap,
}

/// UI-R-063 — flatten `memory_rows`'s per-kind pair-lists into one ordered sequence of
/// hex-editor lines, inserting a `Gap` wherever the previous line and this one are not
/// address-contiguous within the same table kind (a table-kind change is always non-contiguous
/// by construction).
fn memory_layout_lines(kind_rows: &[(Kind, Vec<(u16, u16)>)]) -> Vec<MemoryLine> {
    let mut out = Vec::new();
    let mut prev_end: Option<(Kind, u16)> = None;
    for (kind, pairs) in kind_rows {
        let (unit_per_cell, cells_per_line) = memory_cell_shape(kind.clone());
        let span = unit_per_cell * cells_per_line;
        for (address, cells) in memory_lines(kind.clone(), pairs) {
            let contiguous = prev_end
                .as_ref()
                .map(|(prev_kind, prev_end_addr)| *prev_kind == *kind && *prev_end_addr == address)
                .unwrap_or(true);
            if !out.is_empty() && !contiguous {
                out.push(MemoryLine::Gap);
            }
            prev_end = Some((kind.clone(), address.saturating_add(span)));
            out.push(MemoryLine::Line {
                kind: kind.clone(),
                address,
                cells,
            });
        }
    }
    out
}

/// Whether any of `cell_address`'s `unit_per_cell` constituent raw addresses is MB-R-147
/// recency-active (Shared) as of `now`.
fn memory_cell_recency_active(
    kind: Kind,
    cell_address: u16,
    unit_per_cell: u16,
    records: &[MonitorRecord],
    now: std::time::Instant,
) -> bool {
    (0..unit_per_cell).any(|i| {
        ferrowl_modbus::monitor::recency_active_at(records, kind.clone(), cell_address + i, now)
    })
}

/// UI-R-063 — paint `lines` into `area` of `buf`: per line, a `{address:04x}` prefix, hex cell
/// values (coil-family bytes as `{:02x}`, register-family words as `{:04x}`), then a
/// `|char...|` representation column; a `Gap` line paints its full width in `gap_bg`. A cell
/// covered by an active MB-R-147 recency marker paints `hi`, overriding its value-class color
/// (mirrors `Definition::cell_styles`'s highlight-overrides-base-style precedence).
fn render_memory_layout(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    lines: &[MemoryLine],
    records: &[MonitorRecord],
    now: std::time::Instant,
) {
    use ferrowl_ui::COLOR_SCHEME;
    use ratatui::style::Style;

    for (row, line) in lines.iter().enumerate() {
        let y = area.y.saturating_add(row as u16);
        if y >= area.y + area.height {
            break;
        }
        match line {
            MemoryLine::Gap => {
                buf.set_style(
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    },
                    Style::default().bg(COLOR_SCHEME.gap_bg),
                );
            }
            MemoryLine::Line {
                kind,
                address,
                cells,
            } => {
                let unit_per_cell = memory_cell_shape(kind.clone()).0;
                let mut x = area.x;
                let prefix = format!("{address:04x} ");
                buf.set_string(x, y, &prefix, Style::default().fg(COLOR_SCHEME.border));
                x += prefix.len() as u16;
                let mut chars = String::new();
                for (i, cell) in cells.iter().enumerate() {
                    let cell_address = address.saturating_add((i as u16) * unit_per_cell);
                    let hex = if unit_per_cell == 8 {
                        format!("{:02x}", cell.value)
                    } else {
                        format!("{:04x}", cell.value)
                    };
                    let color = if memory_cell_recency_active(
                        kind.clone(),
                        cell_address,
                        unit_per_cell,
                        records,
                        now,
                    ) {
                        COLOR_SCHEME.hi
                    } else {
                        memory_cell_value_style(cell)
                    };
                    buf.set_string(x, y, &hex, Style::default().fg(color));
                    x += hex.len() as u16 + 1;
                    chars.push(memory_cell_char(cell));
                }
                buf.set_string(
                    x,
                    y,
                    format!("|{chars}|"),
                    Style::default().fg(COLOR_SCHEME.text),
                );
            }
        }
    }
}

// Forward-declared: real app-side construction lands in s8 of the modbus-bus-monitor plan (wiring the 3 construction call sites); already fully implemented and tested here.
#[allow(dead_code)]
pub struct ModbusMonitorModuleView {
    module: ModbusMonitorModule,
    spec: ModuleSpec,
    device: MonitorDeviceConfig,
    /// Every unit id observed so far, sorted, refreshed each tick (UI-R-060).
    unit_ids: Vec<UnitId>,
    /// Index into `unit_ids` of the left panel's current selection.
    selected: usize,
    /// UI-R-062 Messages table for the selected unit id, re-derived each tick from
    /// `module.records()`.
    messages_table: MessageTable,
    /// UI-R-064 Resolved-registers table for the selected unit id, re-derived each tick (Shared
    /// with the Messages table's own re-derivation pattern).
    resolved_table: ResolvedTable,
    overlay: MonitorOverlay,
    view_focused: bool,
    /// Which panel is Tab-focused within this view (UI-R-060/061 parity with OCPP's per-panel
    /// border highlight); only meaningful while `view_focused` is `true`.
    panel_focus: MonitorPanel,
    /// `:compact` toggle (tui/api-contract.md §2.1) — a real row-margin toggle on
    /// `resolved_table.widget` (mirrors `TableView::set_compact`, Shared), not string formatting.
    compact: bool,
    /// `:order [col] [asc|desc]` (tui/api-contract.md §2.1) — sorts the resolved-registers
    /// table by column *index* (resolved from the name once, at `:order` parse time, via
    /// `column_index_for::<ResolvedHeader, 9>`); `None` is definition order. `bool` is
    /// "descending". Re-applied every `refresh()` tick (not just once at `:order` time) so newly
    /// added interpretations stay correctly ordered — a deliberate implementation choice, not
    /// spec-mandated (either satisfies UI-R-064/tui/api-contract.md).
    sort: Option<(usize, bool)>,
}

#[allow(dead_code)] // forward-declared; see struct's note
impl ModbusMonitorModuleView {
    pub fn new(module: ModbusMonitorModule, spec: ModuleSpec, device: MonitorDeviceConfig) -> Self {
        Self {
            module,
            spec,
            device,
            unit_ids: Vec::new(),
            selected: 0,
            messages_table: new_message_table(),
            resolved_table: new_resolved_table(),
            overlay: MonitorOverlay::None,
            view_focused: false,
            panel_focus: MonitorPanel::default(),
            compact: false,
            sort: None,
        }
    }

    fn selected_unit(&self) -> Option<UnitId> {
        self.unit_ids.get(self.selected).copied()
    }

    /// Validate the open `Add` overlay's dialog, scope it to the selected unit id (UI-R-061:
    /// "the dialog never asks for it"), and add it to the module's interpretations (MB-R-145).
    /// No-op if the overlay isn't open, the dialog is invalid, or nothing is selected.
    fn confirm_add(&mut self) {
        let MonitorOverlay::Add(dialog) = &self.overlay else {
            return;
        };
        let Ok((name, def)) = dialog.apply() else {
            return;
        };
        let Some(unit) = self.selected_unit() else {
            return;
        };
        self.module.add_interpretation(unit, name, def);
        self.overlay = MonitorOverlay::None;
    }

    /// Resolve the open `Edit` overlay's dialog and rebuild `spec`/`device`/`module` from it
    /// (MB-R-140), mirroring `:reload`'s "stop the old task, build a fresh module" shape but
    /// without an implicit restart — the user starts the monitor explicitly, same as after any
    /// other setup edit. No-op if the overlay isn't open or the dialog doesn't resolve.
    fn confirm_edit(&mut self) {
        let MonitorOverlay::EditSetup(dialog) = &self.overlay else {
            return;
        };
        let Ok(outcome) = dialog.resolve() else {
            return;
        };
        let device = if let Some((path, device)) = outcome.device {
            self.spec.device = path;
            device
        } else {
            self.device.clone()
        };
        self.spec.name = outcome.values.name;
        self.spec.endpoint = outcome.values.endpoint;
        let mut device = device;
        device.reconnect = Some(outcome.values.reconnect);
        self.module = ModbusMonitorModule::new(&self.spec, &device);
        self.device = device;
        self.overlay = MonitorOverlay::None;
    }

    /// MB-R-148 — `Enter` on a Resolved-registers row (with the panel focused, `s14`) opens the
    /// edit/delete dialog prefilled from that row. No-op if nothing is selected in either table.
    fn open_edit_interpretation(&mut self) {
        let Some(unit) = self.selected_unit() else {
            return;
        };
        let Some(idx) = self.resolved_table.state.table_state().selected() else {
            return;
        };
        // UI-R-064's `:order` may have sorted `resolved_table` out of `interpretations_for`'s
        // definition order, so look up by the selected row's own `name`, not by `idx` into
        // `interpretations_for` directly.
        let Some(row) = self.resolved_table.state.values().get(idx) else {
            return;
        };
        let name = row.name.clone();
        let interpretations = self.interpretations_for(unit);
        let Some((_, def)) = interpretations.iter().find(|(n, _)| **n == name) else {
            return;
        };
        let dialog = EditInterpretationDialog::from_interpretation(&name, def);
        self.overlay = MonitorOverlay::EditInterpretation(Box::new(dialog), name);
    }

    /// MB-R-148 — apply the open `EditInterpretation` overlay's Confirm: edit the interpretation
    /// in place under its (possibly new) name. Never touches `module.table()` (Shared: "neither
    /// operation writes to the bus or otherwise touches the slave's observed-value table").
    /// No-op if the overlay isn't open, the dialog is invalid, or nothing is selected.
    fn confirm_edit_interpretation(&mut self) {
        let MonitorOverlay::EditInterpretation(dialog, original_name) = &self.overlay else {
            return;
        };
        let Ok((new_name, def)) = dialog.apply() else {
            return;
        };
        let Some(unit) = self.selected_unit() else {
            return;
        };
        self.module
            .edit_interpretation(unit, original_name, new_name, def);
        self.overlay = MonitorOverlay::None;
    }

    /// MB-R-148 — apply the open `EditInterpretation` overlay's confirmed Delete: remove the
    /// interpretation outright. Never touches `module.table()` (Shared, see
    /// `confirm_edit_interpretation`). No-op if the overlay isn't open or nothing is selected.
    fn delete_interpretation(&mut self) {
        let MonitorOverlay::EditInterpretation(_, original_name) = &self.overlay else {
            return;
        };
        let Some(unit) = self.selected_unit() else {
            return;
        };
        self.module.remove_interpretation(unit, original_name);
        self.overlay = MonitorOverlay::None;
    }

    /// Interpretations defined for `unit` (MB-R-145), by name, in definition order — the
    /// Resolved-registers table's own real column sort (UI-R-064) applies separately, on the
    /// built `ResolvedRow`s (see `resolved_rows`/`apply_order`).
    fn interpretations_for(&self, unit: UnitId) -> Vec<(&String, &MonitorRegisterDef)> {
        self.module
            .interpretations_for(unit)
            .iter()
            .map(|(name, def)| (name, def))
            .collect()
    }

    /// Whether `unit` has at least one interpretation (MB-R-145) — drives both the
    /// Resolved-registers section's visibility (UI-R-061) and whether `MonitorPanel::Resolved`
    /// is reachable in the Tab cycle (UI-R-065).
    fn has_interpretation(&self, unit: UnitId) -> bool {
        !self.interpretations_for(unit).is_empty()
    }

    /// The selected unit id's `ResolvedRow`s (UI-R-064), definition order unless `self.sort` is
    /// set, in which case sorted by that column index via `cmp_table_entry` (Shared).
    fn resolved_rows(&self, unit: UnitId) -> Vec<ResolvedRow> {
        let table = self.module.table();
        let table = table.read();
        let mut rows: Vec<ResolvedRow> = self
            .interpretations_for(unit)
            .into_iter()
            .map(|(name, def)| resolved_row(name, def, &table, unit))
            .collect();
        if let Some((column, descending)) = self.sort {
            rows.sort_by(|a, b| {
                crate::module::modbus::table::cmp_table_entry(a, b, column, descending)
            });
        }
        rows
    }

    /// Validate `:order`'s optional column argument (tui/api-contract.md §2.1) against the
    /// Resolved-registers table's real columns and set the sort; `Err` for an unrecognized
    /// column name.
    fn apply_order(&mut self, col: &str, desc: bool) -> Result<(), String> {
        match crate::module::modbus::table::column_index_for::<ResolvedHeader, 9>(col) {
            Some(idx) => {
                self.sort = Some((idx, desc));
                Ok(())
            }
            None => Err(format!(
                "unknown column '{col}' (expected one of: {})",
                ResolvedHeader::header().join(", ")
            )),
        }
    }

    /// Memory layout for `unit`, grouped by table kind (MB-R-144), non-empty kinds only.
    fn memory_rows(&self, unit: UnitId) -> Vec<(Kind, Vec<(u16, u16)>)> {
        let table = self.module.table();
        let table = table.read();
        TABLE_KINDS
            .iter()
            .filter_map(|kind| {
                let key = Key::new(SlaveKey {
                    slave_id: unit,
                    kind: kind.clone(),
                });
                let dump = table.dump(&key);
                if dump.is_empty() {
                    None
                } else {
                    Some((kind.clone(), dump))
                }
            })
            .collect()
    }
}

impl ferrowl_ui::traits::SetFocus for ModbusMonitorModuleView {
    fn set_focused(&mut self, focus: bool) {
        self.view_focused = focus;
    }
}

impl ferrowl_ui::traits::IsFocus for ModbusMonitorModuleView {
    fn is_focused(&self) -> bool {
        self.view_focused
    }
}

impl ModuleView for ModbusMonitorModuleView {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        !matches!(self.overlay, MonitorOverlay::None)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ferrowl_ui::COLOR_SCHEME;
        use ratatui::layout::{Constraint, Layout};
        use ratatui::style::Style;
        use ratatui::widgets::{Block, Borders, List, ListItem};

        // Each Tab-cyclable panel's border tracks whether it is the panel-focused one AND the
        // view itself has focus (blue), light gray otherwise — same per-panel highlight pattern
        // as the OCPP module view. The resolved-registers panel is display-only, always gray.
        let style_for = |panel: MonitorPanel| {
            Style::default()
                .fg(if self.view_focused && self.panel_focus == panel {
                    COLOR_SCHEME.hi
                } else {
                    COLOR_SCHEME.border
                })
                .bg(COLOR_SCHEME.bg)
        };
        let units_style = style_for(MonitorPanel::Units);
        let memory_style = style_for(MonitorPanel::Memory);

        let [left_area, right_area] =
            Layout::horizontal([Constraint::Length(12), Constraint::Min(1)]).areas(area);

        let buf = frame.buffer_mut();

        let items: Vec<ListItem> = self
            .unit_ids
            .iter()
            .enumerate()
            .map(|(idx, unit)| {
                let label = format!("{unit}");
                if idx == self.selected {
                    ListItem::new(format!("> {label}"))
                } else {
                    ListItem::new(format!("  {label}"))
                }
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(units_style)
                .title("Units"),
        );
        ratatui::widgets::Widget::render(list, left_area, buf);

        let selected = self.selected_unit();
        let has_interpretation = selected.is_some_and(|u| self.has_interpretation(u));

        let sections: Vec<Constraint> = if has_interpretation {
            vec![
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ]
        } else {
            vec![Constraint::Percentage(50), Constraint::Percentage(50)]
        };
        let section_areas = Layout::vertical(sections).split(right_area);

        // Message table (MB-R-143/146, UI-R-062).
        self.messages_table
            .state
            .set_focused(self.view_focused && self.panel_focus == MonitorPanel::Messages);
        StatefulWidget::render(
            &self.messages_table.widget,
            section_areas[0],
            buf,
            &mut self.messages_table.state,
        );

        // Memory layout, grouped by table kind (MB-R-144).
        let memory_block = Block::default()
            .borders(Borders::ALL)
            .border_style(memory_style)
            .title("Memory layout");
        let memory_inner = memory_block.inner(section_areas[1]);
        ratatui::widgets::Widget::render(memory_block, section_areas[1], buf);
        if let Some(unit) = selected {
            let kind_rows = self.memory_rows(unit);
            let lines = memory_layout_lines(&kind_rows);
            let records = self.module.records().read().records_for(unit);
            render_memory_layout(
                buf,
                memory_inner,
                &lines,
                &records,
                std::time::Instant::now(),
            );
        }

        // Resolved registers (MB-R-145, UI-R-064) — omitted entirely when no interpretation
        // exists for the selected unit id (UI-R-061). Rebuilt live here (not cached in
        // `refresh()`, unlike the Messages table) so a newly added interpretation shows up
        // immediately, without a fresh refresh tick — same invariant the previous
        // `Paragraph`-based render held.
        if has_interpretation && let Some(unit) = selected {
            self.resolved_table
                .state
                .set_values(self.resolved_rows(unit));
            self.resolved_table
                .state
                .set_focused(self.view_focused && self.panel_focus == MonitorPanel::Resolved);
            StatefulWidget::render(
                &self.resolved_table.widget,
                section_areas[2],
                buf,
                &mut self.resolved_table.state,
            );
        }
    }

    fn render_overlay(&mut self, frame: &mut Frame, _area: Rect) {
        let full_area = frame.area();
        match &mut self.overlay {
            MonitorOverlay::Add(dialog) => dialog.render(full_area, frame.buffer_mut()),
            MonitorOverlay::EditSetup(dialog) => dialog.render(full_area, frame.buffer_mut()),
            MonitorOverlay::EditInterpretation(dialog, _) => {
                dialog.render(full_area, frame.buffer_mut())
            }
            MonitorOverlay::None => {}
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let MonitorOverlay::EditSetup(dialog) = &mut self.overlay {
            match code {
                KeyCode::Esc => {
                    self.overlay = MonitorOverlay::None;
                }
                KeyCode::Enter => {
                    self.confirm_edit();
                }
                _ => {
                    let _ = dialog.handle_events(modifiers, code);
                }
            }
            return EventResult::Consumed;
        }
        if let MonitorOverlay::Add(dialog) = &mut self.overlay {
            match code {
                KeyCode::Esc if dialog.add_dialog.is_some() => {
                    dialog.add_dialog = None;
                }
                KeyCode::Esc => {
                    self.overlay = MonitorOverlay::None;
                }
                KeyCode::Enter if dialog.add_dialog.is_some() => {
                    dialog.confirm_add_dialog();
                }
                KeyCode::Enter if dialog.is_confirm_button_focused() => {
                    self.confirm_add();
                }
                KeyCode::Char(' ') => {
                    dialog.handle_space();
                }
                KeyCode::BackTab => {
                    dialog.focus_previous();
                }
                KeyCode::Tab => {
                    dialog.focus_next();
                }
                _ => {
                    let _ = ferrowl_ui::traits::HandleEvents::handle_events(
                        dialog.as_mut(),
                        modifiers,
                        code,
                    );
                }
            }
            return EventResult::Consumed;
        }
        if let MonitorOverlay::EditInterpretation(dialog, _) = &mut self.overlay {
            use crate::module::modbus::dialog::{DeleteConfirmOutcome, route_delete_confirm};
            if dialog.confirm_delete.is_some() {
                match route_delete_confirm(&mut dialog.confirm_delete, modifiers, code) {
                    DeleteConfirmOutcome::Confirmed => {
                        self.delete_interpretation();
                    }
                    DeleteConfirmOutcome::Consumed | DeleteConfirmOutcome::NotActive => {}
                }
                return EventResult::Consumed;
            }
            match code {
                KeyCode::Esc if dialog.add_dialog.is_some() => {
                    dialog.add_dialog = None;
                }
                KeyCode::Esc => {
                    self.overlay = MonitorOverlay::None;
                }
                KeyCode::Enter if dialog.add_dialog.is_some() => {
                    dialog.confirm_add_dialog();
                }
                KeyCode::Enter if dialog.is_confirm_button_focused() => {
                    self.confirm_edit_interpretation();
                }
                KeyCode::Char(' ') => {
                    dialog.handle_space();
                }
                KeyCode::BackTab => {
                    dialog.focus_previous();
                }
                KeyCode::Tab => {
                    dialog.focus_next();
                }
                _ => {
                    let _ = ferrowl_ui::traits::HandleEvents::handle_events(
                        dialog.as_mut(),
                        modifiers,
                        code,
                    );
                }
            }
            return EventResult::Consumed;
        }
        // UI-R-065 — `Resolved` is only reachable while the selected unit id has an
        // interpretation; `next`/`previous` alone don't know that, so skip it here at the call
        // site (Shared's `MonitorPanel` doc comment).
        let resolved_visible = self
            .selected_unit()
            .is_some_and(|u| self.has_interpretation(u));
        match code {
            KeyCode::Tab if modifiers == KeyModifiers::NONE => {
                let mut next = self.panel_focus.next();
                if next == MonitorPanel::Resolved && !resolved_visible {
                    next = next.next();
                }
                self.panel_focus = next;
                EventResult::Consumed
            }
            KeyCode::BackTab => {
                let mut prev = self.panel_focus.previous();
                if prev == MonitorPanel::Resolved && !resolved_visible {
                    prev = prev.previous();
                }
                self.panel_focus = prev;
                EventResult::Consumed
            }
            KeyCode::Up if self.panel_focus == MonitorPanel::Units => {
                self.selected = self.selected.saturating_sub(1);
                EventResult::Consumed
            }
            KeyCode::Down if self.panel_focus == MonitorPanel::Units => {
                if self.selected + 1 < self.unit_ids.len() {
                    self.selected += 1;
                }
                EventResult::Consumed
            }
            _ if self.panel_focus == MonitorPanel::Messages => {
                self.messages_table.state.handle_events(modifiers, code)
            }
            KeyCode::Enter if self.panel_focus == MonitorPanel::Resolved => {
                self.open_edit_interpretation();
                EventResult::Consumed
            }
            _ if self.panel_focus == MonitorPanel::Resolved => {
                self.resolved_table.state.handle_events(modifiers, code)
            }
            _ => EventResult::Unhandled(modifiers, code),
        }
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            {
                let table = self.module.table();
                let table = table.read();
                self.unit_ids = table.unit_ids();
            }
            if self.selected >= self.unit_ids.len() {
                self.selected = self.unit_ids.len().saturating_sub(1);
            }

            let rows = match self.selected_unit() {
                Some(unit) => {
                    let now = std::time::Instant::now();
                    let mut records = self.module.records().read().records_for(unit);
                    records.reverse(); // most-recent-first (UI-R-062)
                    records
                        .iter()
                        .map(|record| message_row(unit, record, now))
                        .collect()
                }
                None => Vec::new(),
            };
            self.messages_table.state.set_values(rows);
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let Some(parsed) = parse_command(&MONITOR_COMMAND_SPECS, cmd) else {
            return Box::pin(std::future::ready(CommandResult::Unhandled));
        };

        match parsed {
            ModbusMonitorCmd::Start => Box::pin(async move {
                let endpoint = self.spec.endpoint.to_string();
                match self
                    .module
                    .start(move |_s: String| async {}, move |_s: String| async {})
                    .await
                {
                    Ok(()) => CommandResult::Handled(Some((
                        Level::Info,
                        format!("Started monitor on {endpoint}"),
                    ))),
                    Err(e) => CommandResult::Handled(Some((
                        Level::Error,
                        format!("Start monitor failed: {e}"),
                    ))),
                }
            }),

            ModbusMonitorCmd::Stop => Box::pin(async move {
                match self.module.stop().await {
                    Ok(()) => CommandResult::Handled(Some((Level::Info, "Stopped monitor".into()))),
                    Err(e) => CommandResult::Handled(Some((
                        Level::Error,
                        format!("Stop monitor failed: {e}"),
                    ))),
                }
            }),

            ModbusMonitorCmd::Restart => Box::pin(async move {
                let endpoint = self.spec.endpoint.to_string();
                let _ = self.module.stop().await;
                match self
                    .module
                    .start(move |_s: String| async {}, move |_s: String| async {})
                    .await
                {
                    Ok(()) => CommandResult::Handled(Some((
                        Level::Info,
                        format!("Restarted monitor on {endpoint}"),
                    ))),
                    Err(e) => CommandResult::Handled(Some((
                        Level::Error,
                        format!("Restart monitor failed: {e}"),
                    ))),
                }
            }),

            ModbusMonitorCmd::Reload => Box::pin(async move {
                if self.spec.device.is_empty() {
                    return CommandResult::Handled(Some((
                        Level::Warning,
                        "No configuration file path configured. Reload aborted.".into(),
                    )));
                }
                let path = self.spec.device.clone();
                let device: MonitorDeviceConfig = match crate::config::load_monitor_device(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        return CommandResult::Handled(Some((
                            Level::Error,
                            format!(":reload failed to load '{path}': {e}"),
                        )));
                    }
                };
                let _ = self.module.stop().await;
                let new_module = ModbusMonitorModule::new(&self.spec, &device);
                self.module = new_module;
                self.device = device;
                if let Err(e) = self
                    .module
                    .start(move |_s: String| async {}, move |_s: String| async {})
                    .await
                {
                    return CommandResult::Handled(Some((
                        Level::Error,
                        format!(":reload start error: {e}"),
                    )));
                }
                CommandResult::Handled(Some((Level::Info, format!(":reload done — '{path}'"))))
            }),

            ModbusMonitorCmd::Edit => {
                let dialog = MonitorSetupDialog::edit(&self.spec.name, &self.spec, &self.device);
                self.overlay = MonitorOverlay::EditSetup(Box::new(dialog));
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }

            ModbusMonitorCmd::Add => {
                self.overlay = MonitorOverlay::Add(Box::new(AddInterpretationDialog::new()));
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }

            ModbusMonitorCmd::Compact => {
                self.compact = !self.compact;
                // Real row-padding toggle (UI-R-064), mirroring `TableView::set_compact`
                // (Shared): 1 row of vertical margin expanded, 0 compact.
                let vertical = if self.compact { 0 } else { 1 };
                self.resolved_table
                    .widget
                    .set_row_margin(ratatui::layout::Margin {
                        vertical,
                        horizontal: 0,
                    });
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }

            ModbusMonitorCmd::WriteDevice(rest) => {
                let path = rest.unwrap_or_else(|| self.spec.device.clone());
                if path.is_empty() {
                    return Box::pin(std::future::ready(CommandResult::Handled(Some((
                        Level::Warning,
                        "No configuration file path configured.".into(),
                    )))));
                }
                let result = save_device_to(&self.device, &path);
                Box::pin(std::future::ready(result))
            }

            ModbusMonitorCmd::Log(None) => Box::pin(std::future::ready(CommandResult::Unhandled)),

            ModbusMonitorCmd::Log(Some(file)) => {
                self.device.log_file = Some(file.clone());
                Box::pin(std::future::ready(CommandResult::Handled(Some((
                    Level::Info,
                    format!("Logging to files based on {file} (':wd' to persist)"),
                )))))
            }

            ModbusMonitorCmd::Order(rest) => {
                let parts: Vec<&str> = rest.as_deref().unwrap_or("").split_whitespace().collect();
                let result = match parts.as_slice() {
                    [] => {
                        self.sort = None;
                        CommandResult::Handled(Some((Level::Info, "Order cleared".to_string())))
                    }
                    [col] | [col, "asc"] => match self.apply_order(col, false) {
                        Ok(()) => CommandResult::Handled(None),
                        Err(e) => CommandResult::Handled(Some((Level::Warning, e))),
                    },
                    [col, "desc"] => match self.apply_order(col, true) {
                        Ok(()) => CommandResult::Handled(None),
                        Err(e) => CommandResult::Handled(Some((Level::Warning, e))),
                    },
                    _ => CommandResult::Unhandled,
                };
                Box::pin(std::future::ready(result))
            }
        }
    }

    fn commands(&self) -> &[CommandDescriptor] {
        static DESCRIPTORS: std::sync::OnceLock<Vec<CommandDescriptor>> =
            std::sync::OnceLock::new();
        DESCRIPTORS.get_or_init(|| MONITOR_COMMAND_SPECS.iter().map(|s| s.descriptor).collect())
    }

    fn log(&self) -> SharedLog {
        self.module.log()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let mut v = serde_json::to_value(&self.spec).ok()?;
        v.as_object_mut()?.insert("type".into(), "modbus".into());
        Some(v)
    }
}

/// The parsed form of every command this view accepts; produced by [`parse_command`] over
/// [`MONITOR_COMMAND_SPECS`]. `:set`/`:script` are simply absent from the table — `parse_command`
/// already falls through to `CommandResult::Unhandled` for anything not listed, which is exactly
/// tui/api-contract.md's "both are unrecognized on this role rather than erroring".
#[allow(dead_code)] // consumed starting s8
enum ModbusMonitorCmd {
    Start,
    Stop,
    Restart,
    Reload,
    Edit,
    Add,
    Compact,
    WriteDevice(Option<String>),
    Log(Option<String>),
    Order(Option<String>),
}

/// Single source for this view's commands: aliases, help row, and parse target per entry.
#[allow(dead_code)] // consumed starting s8
static MONITOR_COMMAND_SPECS: [CommandSpec<ModbusMonitorCmd>; 10] = [
    CommandSpec {
        aliases: &["e", "edit"],
        descriptor: CommandDescriptor {
            name: ":e | :edit",
            description: "edit module setup",
        },
        build: |_| ModbusMonitorCmd::Edit,
    },
    CommandSpec {
        aliases: &["a", "add"],
        descriptor: CommandDescriptor {
            name: ":a | :add",
            description: "add register interpretation",
        },
        build: |_| ModbusMonitorCmd::Add,
    },
    CommandSpec {
        aliases: &["start"],
        descriptor: CommandDescriptor {
            name: ":start",
            description: "start module",
        },
        build: |_| ModbusMonitorCmd::Start,
    },
    CommandSpec {
        aliases: &["stop"],
        descriptor: CommandDescriptor {
            name: ":stop",
            description: "stop module",
        },
        build: |_| ModbusMonitorCmd::Stop,
    },
    CommandSpec {
        aliases: &["restart"],
        descriptor: CommandDescriptor {
            name: ":restart",
            description: "restart module",
        },
        build: |_| ModbusMonitorCmd::Restart,
    },
    CommandSpec {
        aliases: &["reload"],
        descriptor: CommandDescriptor {
            name: ":reload",
            description: "reload device config",
        },
        build: |_| ModbusMonitorCmd::Reload,
    },
    CommandSpec {
        aliases: &["compact"],
        descriptor: CommandDescriptor {
            name: ":compact",
            description: "toggle compact mode",
        },
        build: |_| ModbusMonitorCmd::Compact,
    },
    CommandSpec {
        aliases: &["wd", "write-device"],
        descriptor: CommandDescriptor {
            name: ":wd | :write-device [path]",
            description: "save device config",
        },
        build: |rest| ModbusMonitorCmd::WriteDevice(rest.map(str::to_string)),
    },
    CommandSpec {
        aliases: &["log"],
        descriptor: CommandDescriptor {
            name: ":log <file>",
            description: "set log file",
        },
        build: |rest| ModbusMonitorCmd::Log(rest.map(str::to_string)),
    },
    CommandSpec {
        aliases: &["order"],
        descriptor: CommandDescriptor {
            name: ":order [col] [asc|desc]",
            description: "sort table by column",
        },
        build: |rest| ModbusMonitorCmd::Order(rest.map(str::to_string)),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Endpoint, Role};
    use ferrowl_modbus::UnitId;
    use ferrowl_ui::widgets::TableEntry as TableEntryTrait;

    fn spec() -> ModuleSpec {
        ModuleSpec {
            name: "mon1".to_string(),
            device: String::new(),
            role: Role::Monitor,
            endpoint: Endpoint::Rtu {
                path: "/dev/none".to_string(),
                baud_rate: 9600,
                parity: None,
                data_bits: None,
                stop_bits: None,
            },
        }
    }

    fn device() -> MonitorDeviceConfig {
        MonitorDeviceConfig::default()
    }

    fn view() -> ModbusMonitorModuleView {
        let module = ModbusMonitorModule::new(&spec(), &device());
        ModbusMonitorModuleView::new(module, spec(), device())
    }

    /// UI-R-060 — the left panel's `unit_ids` refresh from the observed table, live.
    #[tokio::test]
    async fn ut_refresh_populates_unit_ids_from_observed_table() {
        let mut v = view();
        {
            let table = v.module.table();
            let mut table = table.write();
            table.write_words(
                Key::new(SlaveKey {
                    slave_id: UnitId(3),
                    kind: Kind::HoldingRegister,
                }),
                0,
                &[1],
            );
        }
        v.refresh().await;
        assert_eq!(v.unit_ids, vec![UnitId(3)]);
    }

    /// UI-R-061 — the resolved-registers section is omitted entirely from the rendered buffer
    /// when no interpretation exists for the selected unit id, and reappears once one does.
    #[test]
    fn ut_resolved_registers_section_hidden_when_no_interpretation_for_selected_unit() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        let contents =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut acc, cell| {
                    acc.push_str(cell.symbol());
                    acc
                });
        assert!(!contents.contains("Resolved registers"));

        v.module.add_interpretation(
            UnitId(3),
            "power".to_string(),
            MonitorRegisterDef {
                slave_id: 3,
                kind: Kind::HoldingRegister,
                address: Some(0),
                is_virtual: false,
                value_type: crate::config::device::ValueType::U16,
                endian: Default::default(),
                word_order: Default::default(),
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: Default::default(),
                values: vec![],
                description: String::new(),
                default: None,
            },
        );

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        let contents =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut acc, cell| {
                    acc.push_str(cell.symbol());
                    acc
                });
        assert!(contents.contains("Resolved registers"));
    }

    /// Regression — panel titles must not show requirement IDs; they're not useful to the
    /// application's user. Previously "Messages (MB-R-143)", "Memory layout (MB-R-144)" and
    /// "Resolved registers (MB-R-145)" leaked the spec IDs into the rendered UI.
    #[test]
    fn ut_panel_titles_have_no_requirement_ids() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));
        let contents = buffer_text(&mut v);
        assert!(contents.contains("Messages"));
        assert!(contents.contains("Memory layout"));
        assert!(contents.contains("Resolved registers"));
        assert!(!contents.contains("MB-R-143"));
        assert!(!contents.contains("MB-R-144"));
        assert!(!contents.contains("MB-R-145"));
    }

    /// Regression — every panel's border must track the view's own focus (blue when focused,
    /// light gray otherwise), same as every other module content view; previously every border
    /// used the terminal's plain default color with no focus differentiation at all.
    #[test]
    fn ut_panel_borders_track_view_focus() {
        use ferrowl_ui::COLOR_SCHEME;
        use ferrowl_ui::traits::SetFocus;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut v = view();
        v.set_focused(false);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(0, 0)].fg,
            COLOR_SCHEME.border,
            "unfocused panel border must use the unfocused border color"
        );

        v.set_focused(true);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(0, 0)].fg,
            COLOR_SCHEME.hi,
            "focused panel border must use the highlight color"
        );
    }

    /// Regression — previously every panel shared one `panel_style` driven only by
    /// `view_focused`, so all panels turned blue together with no way to tell which one had
    /// input focus. Now exactly one Tab-cyclable panel is highlighted at a time, defaulting to
    /// Units, and Tab/BackTab cycle Units -> Messages -> Memory -> Units (and back).
    #[test]
    fn ut_tab_cycles_panel_focus_units_messages_memory_and_back() {
        use ferrowl_ui::COLOR_SCHEME;
        use ferrowl_ui::traits::SetFocus;

        let mut v = view();
        v.set_focused(true);
        assert_eq!(v.panel_focus, MonitorPanel::Units);

        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(v.panel_focus, MonitorPanel::Messages);

        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(v.panel_focus, MonitorPanel::Memory);

        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(
            v.panel_focus,
            MonitorPanel::Units,
            "Tab must wrap back to Units"
        );

        v.handle_events(KeyModifiers::NONE, KeyCode::BackTab);
        assert_eq!(
            v.panel_focus,
            MonitorPanel::Memory,
            "BackTab must cycle in reverse"
        );

        // Only the currently panel-focused block's top-left border cell is highlighted.
        v.panel_focus = MonitorPanel::Units;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(0, 0)].fg,
            COLOR_SCHEME.hi,
            "Units panel border must be highlighted while it is panel-focused"
        );
    }

    /// tui/api-contract.md's monitor command table — `:set`/`:script` are simply absent, so
    /// both fall through `parse_command` to `Unhandled` rather than erroring.
    #[tokio::test]
    async fn ut_set_and_script_commands_unrecognized() {
        let mut v = view();
        assert!(matches!(
            v.handle_command("set foo 1").await,
            CommandResult::Unhandled
        ));
        assert!(matches!(
            v.handle_command("script").await,
            CommandResult::Unhandled
        ));
    }

    /// Every entry in `MONITOR_COMMAND_SPECS` routes through `handle_command` without panicking.
    #[tokio::test]
    async fn ut_every_monitor_command_spec_has_matching_handler() {
        let mut v = view();
        for spec in &MONITOR_COMMAND_SPECS {
            for alias in spec.aliases {
                let _ = v.handle_command(alias).await;
            }
        }
    }

    /// `commands()`/`log()` delegate to the command table and the module's own log ring.
    #[test]
    fn ut_monitor_view_commands_and_log_delegate() {
        let v = view();
        assert_eq!(v.commands().len(), MONITOR_COMMAND_SPECS.len());
        let _log: SharedLog = v.log();
    }

    /// UI-R-061 — `:add` scopes the new interpretation to the currently selected unit id; the
    /// dialog never asks for one, and other unit ids' interpretation sets are untouched.
    #[tokio::test]
    async fn ut_add_command_scopes_new_interpretation_to_selected_unit_id() {
        use crate::module::modbus::dialog::set_input;

        let mut v = view();
        v.unit_ids = vec![UnitId(1), UnitId(3)];
        v.selected = 1; // unit 3

        v.handle_command("add").await;
        let MonitorOverlay::Add(dialog) = &mut v.overlay else {
            panic!(":add did not open the interpretation dialog");
        };
        set_input(&mut dialog.label, "power");
        set_input(&mut dialog.address, "10");
        v.confirm_add();

        assert_eq!(v.interpretations_for(UnitId(3)).len(), 1);
        assert_eq!(v.interpretations_for(UnitId(3))[0].0, "power");
        assert!(v.interpretations_for(UnitId(1)).is_empty());
        assert!(matches!(v.overlay, MonitorOverlay::None));
    }

    /// MB-R-145 — a newly added interpretation is reflected in the resolved-registers section
    /// immediately, without a fresh unit-id selection round-trip.
    #[tokio::test]
    async fn ut_resolved_registers_table_reflects_new_interpretation_immediately() {
        use crate::module::modbus::dialog::set_input;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;

        v.handle_command("add").await;
        let MonitorOverlay::Add(dialog) = &mut v.overlay else {
            panic!(":add did not open the interpretation dialog");
        };
        set_input(&mut dialog.label, "power");
        set_input(&mut dialog.address, "10");
        v.confirm_add();

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        let contents =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut acc, cell| {
                    acc.push_str(cell.symbol());
                    acc
                });
        assert!(contents.contains("Resolved registers"));
        assert!(contents.contains("power"));
    }

    /// MB-R-140 — `:edit` opens the setup dialog prefilled from the current spec, and confirming
    /// it rebuilds `spec`/`device`/`module` from the resolved values.
    #[tokio::test]
    async fn ut_edit_command_opens_setup_dialog_prefilled_and_reconfigures_on_confirm() {
        let mut v = view();
        v.handle_command("edit").await;
        let MonitorOverlay::EditSetup(dialog) = &v.overlay else {
            panic!(":edit did not open the setup dialog");
        };
        assert_eq!(dialog.path.state.input(), "/dev/none");

        let MonitorOverlay::EditSetup(dialog) = &mut v.overlay else {
            unreachable!()
        };
        super::super::setup_dialog::set_input(&mut dialog.name, "renamed");
        v.confirm_edit();

        assert_eq!(v.spec.name, "renamed");
        assert!(matches!(v.overlay, MonitorOverlay::None));
    }

    fn def(address: u16, description: &str) -> MonitorRegisterDef {
        MonitorRegisterDef {
            slave_id: 3,
            kind: Kind::HoldingRegister,
            address: Some(address),
            is_virtual: false,
            value_type: crate::config::device::ValueType::U16,
            endian: Default::default(),
            word_order: Default::default(),
            resolution: 1.0,
            bitmask: None,
            length: 1,
            alignment: Default::default(),
            values: vec![],
            description: description.to_string(),
            default: None,
        }
    }

    fn buffer_text(v: &mut ModbusMonitorModuleView) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .fold(String::new(), |mut acc, cell| {
                acc.push_str(cell.symbol());
                acc
            })
    }

    /// tui/api-contract.md §2.1 — `:compact` toggles the resolved-registers section between the
    /// default (expanded, description shown) layout and the compact (name + value only) layout.
    #[tokio::test]
    async fn ut_compact_command_toggles_resolved_table_row_margin() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));

        assert!(!v.compact);
        assert_eq!(v.resolved_table.widget.row_margin().vertical, 1);

        v.handle_command("compact").await;
        assert!(v.compact);
        assert_eq!(v.resolved_table.widget.row_margin().vertical, 0);

        v.handle_command("compact").await;
        assert!(!v.compact);
        assert_eq!(v.resolved_table.widget.row_margin().vertical, 1);
    }

    /// tui/api-contract.md §2.1/UI-R-064 — `:order <col> [asc|desc]` sorts the
    /// Resolved-registers table by the real column index; bare `:order` clears the sort back to
    /// definition order.
    #[tokio::test]
    async fn ut_order_command_sorts_resolved_table_by_column_index() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "low".to_string(), def(1, "low addr"));
        v.module
            .add_interpretation(UnitId(3), "high".to_string(), def(99, "high addr"));

        v.handle_command("order address desc").await;
        buffer_text(&mut v); // renders, which rebuilds `resolved_table`'s rows under the sort
        let names: Vec<String> = v
            .resolved_table
            .state
            .values()
            .iter()
            .map(|r| r.values()[0].clone())
            .collect();
        assert_eq!(names, vec!["high".to_string(), "low".to_string()]);

        v.handle_command("order address asc").await;
        buffer_text(&mut v);
        let names: Vec<String> = v
            .resolved_table
            .state
            .values()
            .iter()
            .map(|r| r.values()[0].clone())
            .collect();
        assert_eq!(names, vec!["low".to_string(), "high".to_string()]);

        v.handle_command("order").await;
        assert!(v.sort.is_none());
    }

    /// tui/api-contract.md §2.1 — an unrecognized `:order` column is reported (Warning), not
    /// silently accepted, and leaves the current sort untouched.
    #[tokio::test]
    async fn ut_order_command_rejects_unknown_column() {
        let mut v = view();
        let result = v.handle_command("order bogus").await;
        assert!(matches!(
            result,
            CommandResult::Handled(Some((Level::Warning, _)))
        ));
        assert!(v.sort.is_none());
    }

    /// Write a word into `unit`'s observed table so `refresh()`'s own `unit_ids` re-derivation
    /// (which runs before the Messages table re-derivation) keeps `unit` selectable, matching
    /// `ut_refresh_populates_unit_ids_from_observed_table`'s own fixture pattern.
    fn seed_unit(v: &ModbusMonitorModuleView, unit: UnitId) {
        v.module.table().write().write_words(
            Key::new(SlaveKey {
                slave_id: unit,
                kind: Kind::HoldingRegister,
            }),
            0,
            &[0],
        );
    }

    fn shaped_record(
        status: RecordStatus,
        operation: ferrowl_modbus::FunctionCode,
        shape: Option<ferrowl_modbus::monitor::TableShape>,
        age: std::time::Duration,
    ) -> MonitorRecord {
        MonitorRecord {
            timestamp: std::time::Instant::now() - age,
            status,
            operation,
            shape,
        }
    }

    fn shape(
        kind: Kind,
        address: u16,
        quantity: u16,
        write_address: Option<u16>,
        write_quantity: Option<u16>,
        values: Vec<u16>,
    ) -> ferrowl_modbus::monitor::TableShape {
        ferrowl_modbus::monitor::TableShape {
            kind,
            address,
            quantity,
            write_address,
            write_quantity,
            values,
        }
    }

    /// UI-R-062 — one row per captured record, most-recent-first, in the fixed 7-column order
    /// (Time, Status, Slave, Operation, Address, Quantity, Values/Payload).
    #[tokio::test]
    async fn ut_messages_table_renders_records_most_recent_first_in_fixed_column_order() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        let older = shaped_record(
            RecordStatus::Ok,
            ferrowl_modbus::FunctionCode::ReadHoldingRegisters,
            Some(shape(Kind::HoldingRegister, 10, 1, None, None, vec![7])),
            std::time::Duration::from_secs(5),
        );
        let newer = shaped_record(
            RecordStatus::Ok,
            ferrowl_modbus::FunctionCode::WriteSingleRegister,
            Some(shape(Kind::HoldingRegister, 20, 1, None, None, vec![9])),
            std::time::Duration::from_secs(1),
        );
        v.module.records().write().push(UnitId(3), older);
        v.module.records().write().push(UnitId(3), newer);

        v.refresh().await;

        let rows = v.messages_table.state.values();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values()[3], "WriteSingleRegister");
        assert_eq!(rows[0].values()[4], "20");
        assert_eq!(rows[1].values()[3], "ReadHoldingRegisters");
        assert_eq!(rows[1].values()[4], "10");
    }

    /// UI-R-062 — a register-shaped record's Values/Payload renders 4-digit lowercase hex per
    /// word.
    #[tokio::test]
    async fn ut_messages_table_formats_register_payload_as_hex_words() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        v.module.records().write().push(
            UnitId(3),
            shaped_record(
                RecordStatus::Ok,
                ferrowl_modbus::FunctionCode::ReadHoldingRegisters,
                Some(shape(
                    Kind::HoldingRegister,
                    0,
                    2,
                    None,
                    None,
                    vec![0x00AB, 0x1234],
                )),
                std::time::Duration::from_millis(100),
            ),
        );

        v.refresh().await;

        let rows = v.messages_table.state.values();
        assert_eq!(rows[0].values()[6], "[00ab 1234]");
    }

    /// UI-R-062 — a coil-shaped record's Values/Payload renders one digit per bit.
    #[tokio::test]
    async fn ut_messages_table_formats_coil_payload_as_bit_digits() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        v.module.records().write().push(
            UnitId(3),
            shaped_record(
                RecordStatus::Ok,
                ferrowl_modbus::FunctionCode::ReadCoils,
                Some(shape(Kind::Coil, 0, 3, None, None, vec![1, 0, 1])),
                std::time::Duration::from_millis(100),
            ),
        );

        v.refresh().await;

        let rows = v.messages_table.state.values();
        assert_eq!(rows[0].values()[6], "[1 0 1]");
    }

    /// UI-R-062 — a record whose operation isn't one of the 9 table-shaping ops renders empty
    /// Address/Quantity/Values-Payload columns.
    #[tokio::test]
    async fn ut_messages_table_non_table_shaping_operation_has_empty_shape_columns() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        v.module.records().write().push(
            UnitId(3),
            shaped_record(
                RecordStatus::Ok,
                ferrowl_modbus::FunctionCode::MaskWriteRegister,
                None,
                std::time::Duration::from_millis(100),
            ),
        );

        v.refresh().await;

        let rows = v.messages_table.state.values();
        assert_eq!(rows[0].values()[4], "");
        assert_eq!(rows[0].values()[5], "");
        assert_eq!(rows[0].values()[6], "");
    }

    /// Edge-cases.md's Monitor boundaries row — `ReadWriteMultipleRegisters`'s shape carries both
    /// its own read and write address/quantity pairs; the table renders them slash-separated.
    #[tokio::test]
    async fn ut_messages_table_read_write_multiple_registers_renders_slash_separated_address_and_quantity()
     {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        v.module.records().write().push(
            UnitId(3),
            shaped_record(
                RecordStatus::Ok,
                ferrowl_modbus::FunctionCode::ReadWriteMultipleRegisters,
                Some(shape(
                    Kind::HoldingRegister,
                    10,
                    2,
                    Some(50),
                    Some(2),
                    vec![11, 22],
                )),
                std::time::Duration::from_millis(100),
            ),
        );

        v.refresh().await;

        let rows = v.messages_table.state.values();
        assert_eq!(rows[0].values()[4], "10/50");
        assert_eq!(rows[0].values()[5], "2/2");
    }

    /// UI-R-062 (horizontal-overflow scrolling reuses `TableState::handle_events` unchanged,
    /// Shared) — `Left`/`Right` reach the Messages table's own scroll handling when it's the
    /// panel-focused one, not the view's own unhandled fallback.
    #[test]
    fn ut_messages_panel_routes_left_right_to_table_horizontal_scroll() {
        let mut v = view();
        v.panel_focus = MonitorPanel::Messages;
        let result = v.handle_events(KeyModifiers::NONE, KeyCode::Right);
        assert!(matches!(result, EventResult::Consumed));
    }

    /// UI-R-063 — two far-apart observed addresses produce only their own two lines, not every
    /// unobserved line in between.
    #[test]
    fn ut_memory_layout_omits_unobserved_lines_renders_observed_ones() {
        let lines = memory_lines(Kind::HoldingRegister, &[(0, 1), (20, 2)]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, 0);
        assert_eq!(lines[1].0, 16); // address 20's line starts at cell 16 (8 words/line)
    }

    /// UI-R-063 — within an otherwise-observed line, a cell with no observed constituent
    /// bit/word renders unobserved, not a silent zero.
    #[test]
    fn ut_memory_layout_unobserved_cell_renders_as_dim_placeholder_not_zero() {
        let lines = memory_lines(Kind::HoldingRegister, &[(0, 7)]);
        let cells = &lines[0].1;
        assert!(cells[0].observed);
        assert_eq!(cells[0].value, 7);
        assert!(!cells[3].observed, "address 3 was never observed");
        assert_eq!(cells[3].value, 0);
        assert_eq!(
            memory_cell_value_style(&cells[3]),
            ferrowl_ui::COLOR_SCHEME.placeholder
        );
    }

    /// UI-R-063 — value-class coloring: unobserved/observed-zero is neutral, observed
    /// printable-ASCII is normal text, any other observed non-zero value is flagged.
    #[test]
    fn ut_memory_layout_value_class_coloring_zero_ascii_other() {
        use ferrowl_ui::COLOR_SCHEME;
        let unobserved = MemoryCell {
            observed: false,
            value: 0,
        };
        let zero = MemoryCell {
            observed: true,
            value: 0,
        };
        let ascii = MemoryCell {
            observed: true,
            value: b'A' as u16,
        };
        let other = MemoryCell {
            observed: true,
            value: 0x0100, // low byte 0x00, not printable
        };
        assert_eq!(
            memory_cell_value_style(&unobserved),
            COLOR_SCHEME.placeholder
        );
        assert_eq!(memory_cell_value_style(&zero), COLOR_SCHEME.placeholder);
        assert_eq!(memory_cell_value_style(&ascii), COLOR_SCHEME.text);
        assert_eq!(memory_cell_value_style(&other), COLOR_SCHEME.warning);
    }

    /// MB-R-147/UI-R-063 — a cell's active recency marker paints `hi`, overriding its
    /// value-class color; once the marker lapses (>2s old), the value-class color shows again.
    #[test]
    fn ut_memory_layout_recency_marker_overrides_value_class_color_while_active() {
        use ferrowl_ui::COLOR_SCHEME;
        use ratatui::buffer::Buffer;

        let area = Rect::new(0, 0, 40, 2);
        let lines = memory_layout_lines(&[(Kind::HoldingRegister, vec![(0, 1)])]);
        let now = std::time::Instant::now();

        let fresh = shaped_record(
            RecordStatus::Ok,
            ferrowl_modbus::FunctionCode::ReadHoldingRegisters,
            Some(shape(Kind::HoldingRegister, 0, 1, None, None, vec![1])),
            std::time::Duration::from_millis(100),
        );
        let mut buf = Buffer::empty(area);
        render_memory_layout(&mut buf, area, &lines, std::slice::from_ref(&fresh), now);
        // prefix "0000 " is 5 chars, so the first hex cell (value=1, non-printable low byte)
        // lands at x=5.
        assert_eq!(buf[(5, 0)].fg, COLOR_SCHEME.hi);

        let stale = shaped_record(
            RecordStatus::Ok,
            ferrowl_modbus::FunctionCode::ReadHoldingRegisters,
            Some(shape(Kind::HoldingRegister, 0, 1, None, None, vec![1])),
            std::time::Duration::from_secs(3),
        );
        let mut buf2 = Buffer::empty(area);
        render_memory_layout(&mut buf2, area, &lines, std::slice::from_ref(&stale), now);
        assert_eq!(buf2[(5, 0)].fg, COLOR_SCHEME.warning);
    }

    /// UI-R-063 — a blank line separates two non-address-contiguous lines, painted in
    /// `gap_bg` across the full width; two contiguous lines get no gap between them.
    #[test]
    fn ut_memory_layout_gap_separator_between_non_adjacent_lines_uses_gap_bg() {
        use ferrowl_ui::COLOR_SCHEME;
        use ratatui::buffer::Buffer;

        let area = Rect::new(0, 0, 40, 3);
        let lines = memory_layout_lines(&[(Kind::HoldingRegister, vec![(0, 1), (20, 2)])]);
        assert_eq!(
            lines.len(),
            3,
            "two non-adjacent lines plus one gap between them"
        );
        assert!(matches!(lines[1], MemoryLine::Gap));

        let mut buf = Buffer::empty(area);
        render_memory_layout(&mut buf, area, &lines, &[], std::time::Instant::now());
        assert_eq!(buf[(0, 1)].bg, COLOR_SCHEME.gap_bg);
        assert_eq!(buf[(39, 1)].bg, COLOR_SCHEME.gap_bg);
        assert_ne!(buf[(0, 0)].bg, COLOR_SCHEME.gap_bg);
    }

    /// UI-R-063 (regression on `memory_rows`'s existing filter, Shared) — a table kind with no
    /// observed traffic for the selected unit is omitted entirely, not rendered as an empty
    /// section.
    #[tokio::test]
    async fn ut_memory_layout_omits_table_kind_with_no_traffic_entirely() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        let kind_rows = v.memory_rows(UnitId(3));
        assert_eq!(kind_rows.len(), 1);
        assert_eq!(kind_rows[0].0, Kind::HoldingRegister);
    }

    /// UI-R-063 — coil-family kinds pack 8 bits per byte-cell, MSB-first, 16 bytes per line.
    #[test]
    fn ut_memory_layout_coil_packs_8_bits_per_byte_16_bytes_per_line() {
        // Bits 0 and 7 of the first byte set; bit 8 (second byte) also set.
        let pairs = vec![(0, 1), (7, 1), (8, 1)];
        let lines = memory_lines(Kind::Coil, &pairs);
        assert_eq!(lines.len(), 1);
        let (start, cells) = &lines[0];
        assert_eq!(*start, 0);
        assert_eq!(cells.len(), 16, "16 bytes per coil-family line");
        // bit 0 -> MSB (0x80), bit 7 -> LSB (0x01)
        assert_eq!(cells[0].value, 0b1000_0001);
        assert!(cells[0].observed);
        assert_eq!(cells[1].value, 0b1000_0000);
        assert!(cells[1].observed);
        assert!(!cells[2].observed);
    }

    /// UI-R-063 — register-family kinds are 1 word per cell, 8 words per line.
    #[test]
    fn ut_memory_layout_register_8_addresses_per_line() {
        let lines = memory_lines(Kind::HoldingRegister, &[(0, 10), (7, 20)]);
        assert_eq!(lines.len(), 1);
        let (start, cells) = &lines[0];
        assert_eq!(*start, 0);
        assert_eq!(cells.len(), 8, "8 words per register-family line");
        assert_eq!(cells[0].value, 10);
        assert_eq!(cells[7].value, 20);
        assert!(!cells[3].observed);
    }

    /// UI-R-064 — the Resolved-registers table has exactly `TableHeader`'s 9 non-Slave-ID/Access
    /// columns, in order.
    #[test]
    fn ut_resolved_registers_table_has_expected_9_columns_in_order() {
        assert_eq!(
            ResolvedHeader::header(),
            [
                "Name",
                "Description",
                "Address",
                "Kind",
                "Format",
                "Length",
                "Resolution",
                "Value",
                "Raw Value",
            ]
        );
    }

    /// UI-R-064 — the Raw Value column renders 4-digit lowercase hex per observed word, same
    /// convention as the Messages table's payload column.
    #[tokio::test]
    async fn ut_resolved_registers_table_raw_value_renders_hex_words() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module.table().write().write_words(
            Key::new(SlaveKey {
                slave_id: UnitId(3),
                kind: Kind::HoldingRegister,
            }),
            10,
            &[5],
        );
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));

        buffer_text(&mut v);
        let rows = v.resolved_table.state.values();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values()[8], "[0005]");
    }

    /// UI-R-065 — Tab cycles all 4 panels (Units→Messages→Memory→Resolved→Units) when the
    /// selected unit id has an interpretation.
    #[test]
    fn ut_tab_cycles_all_4_panels_when_resolved_registers_visible() {
        let mut v = view();
        v.set_focused(true);
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));

        assert_eq!(v.panel_focus, MonitorPanel::Units);
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(v.panel_focus, MonitorPanel::Messages);
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(v.panel_focus, MonitorPanel::Memory);
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(v.panel_focus, MonitorPanel::Resolved);
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(
            v.panel_focus,
            MonitorPanel::Units,
            "Tab must wrap back to Units"
        );
    }

    /// UI-R-065 — the Resolved-registers panel is skipped in the Tab cycle when the selected
    /// unit id has no interpretation (hidden, per UI-R-061).
    #[test]
    fn ut_tab_skips_resolved_registers_panel_when_hidden() {
        let mut v = view();
        v.set_focused(true);
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        // No interpretation added: Resolved stays hidden.

        v.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> Messages
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> Memory
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab); // would be Resolved, skipped -> Units
        assert_eq!(v.panel_focus, MonitorPanel::Units);
    }

    /// UI-R-065 — each panel's selection/scroll is independent: switching panel focus away and
    /// back leaves the Messages table's own row selection untouched.
    #[tokio::test]
    async fn ut_panel_focus_switch_preserves_each_panels_own_selection() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        seed_unit(&v, UnitId(3));
        for i in 0..3 {
            v.module.records().write().push(
                UnitId(3),
                shaped_record(
                    RecordStatus::Ok,
                    ferrowl_modbus::FunctionCode::ReadHoldingRegisters,
                    Some(shape(Kind::HoldingRegister, i, 1, None, None, vec![i])),
                    std::time::Duration::from_secs(1),
                ),
            );
        }
        v.refresh().await;
        v.panel_focus = MonitorPanel::Messages;
        v.messages_table.state.select_index(2);
        assert_eq!(v.messages_table.state.table_state().selected(), Some(2));

        v.panel_focus = MonitorPanel::Memory;
        v.panel_focus = MonitorPanel::Messages;

        assert_eq!(
            v.messages_table.state.table_state().selected(),
            Some(2),
            "switching panel focus away and back must not disturb the Messages table's own selection"
        );
    }

    /// MB-R-148 — `Enter` on the Resolved-registers panel, with a row selected, opens
    /// `MonitorOverlay::EditInterpretation` prefilled from that row (by name, not raw table
    /// index, since `:order` may have reordered the displayed rows away from definition order).
    #[tokio::test]
    async fn ut_enter_on_resolved_row_opens_edit_dialog_prefilled() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));
        buffer_text(&mut v); // renders, which populates `resolved_table`'s rows
        v.panel_focus = MonitorPanel::Resolved;

        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);

        let MonitorOverlay::EditInterpretation(dialog, original_name) = &v.overlay else {
            panic!("Enter on the Resolved panel did not open the edit-interpretation dialog");
        };
        assert_eq!(original_name, "power");
        assert_eq!(dialog.label.state.input(), "power");
        assert_eq!(dialog.description.state.input(), "Active power draw");
        assert_eq!(dialog.address.state.input(), "10");
    }

    /// MB-R-148 — confirming the edit dialog replaces the interpretation in place, under a
    /// (possibly new) name, mirroring `s11`'s module-level `edit_interpretation` test one layer
    /// up, through the dialog/view.
    #[tokio::test]
    async fn ut_confirm_edit_interpretation_replaces_in_place_under_new_name() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));
        buffer_text(&mut v);
        v.panel_focus = MonitorPanel::Resolved;
        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);

        {
            let MonitorOverlay::EditInterpretation(dialog, _) = &mut v.overlay else {
                panic!("edit-interpretation dialog did not open");
            };
            super::super::setup_dialog::set_input(&mut dialog.label, "power2");
        }
        // Same as `ut_add_command_scopes_new_interpretation_to_selected_unit_id`'s own
        // `v.confirm_add()`: call the confirming method directly rather than routing the
        // Confirm-button focus/keypress through `handle_events`.
        v.confirm_edit_interpretation();

        assert!(matches!(v.overlay, MonitorOverlay::None));
        let interpretations = v.module.interpretations_for(UnitId(3));
        assert_eq!(interpretations.len(), 1);
        assert_eq!(interpretations[0].0, "power2");
        assert_eq!(interpretations[0].1.description, "Active power draw");
    }

    /// MB-R-148 — the Delete flow removes the interpretation outright, gated by the dialog's own
    /// `confirm_delete` popup (Space on Delete opens it, Enter on its DELETE button confirms).
    #[tokio::test]
    async fn ut_delete_interpretation_removes_via_confirm_delete_flow() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));
        buffer_text(&mut v);
        v.panel_focus = MonitorPanel::Resolved;
        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);

        {
            let MonitorOverlay::EditInterpretation(dialog, _) = &mut v.overlay else {
                panic!("edit-interpretation dialog did not open");
            };
            dialog.open_confirm_delete();
            assert!(dialog.confirm_delete.is_some());
        }
        // The confirm-delete popup defaults to CANCEL focused; Tab to DELETE, then Enter.
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);

        assert!(matches!(v.overlay, MonitorOverlay::None));
        assert!(v.module.interpretations_for(UnitId(3)).is_empty());
    }

    /// MB-R-148 — neither edit nor delete ever touches `module.table()` (the slave's observed-
    /// value table): a value written there survives both operations unchanged.
    #[tokio::test]
    async fn ut_edit_and_delete_interpretation_never_touch_observed_table() {
        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;
        v.module
            .add_interpretation(UnitId(3), "power".to_string(), def(10, "Active power draw"));
        let key = Key::new(SlaveKey {
            slave_id: UnitId(3),
            kind: Kind::HoldingRegister,
        });
        v.module.table().write().write_words(key.clone(), 10, &[42]);
        let observed_before = v.module.table().read().read_words(&key, 10, 1);

        buffer_text(&mut v);
        v.panel_focus = MonitorPanel::Resolved;
        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        {
            let MonitorOverlay::EditInterpretation(dialog, _) = &mut v.overlay else {
                panic!("edit-interpretation dialog did not open");
            };
            super::super::setup_dialog::set_input(&mut dialog.label, "power2");
        }
        v.confirm_edit_interpretation();
        assert!(
            matches!(v.overlay, MonitorOverlay::None),
            "edit did not confirm"
        );
        assert_eq!(v.module.interpretations_for(UnitId(3))[0].0, "power2");
        assert_eq!(
            v.module.table().read().read_words(&key, 10, 1),
            observed_before
        );

        buffer_text(&mut v);
        v.panel_focus = MonitorPanel::Resolved;
        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        {
            let MonitorOverlay::EditInterpretation(dialog, _) = &mut v.overlay else {
                panic!("edit-interpretation dialog did not open");
            };
            dialog.open_confirm_delete();
        }
        v.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        v.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(v.module.interpretations_for(UnitId(3)).is_empty());
        assert_eq!(
            v.module.table().read().read_words(&key, 10, 1),
            observed_before
        );
    }
}
