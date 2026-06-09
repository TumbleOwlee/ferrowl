use crate::config::device::NamedValue;
use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_reg::Register;
use ferrowl_ui::{
    state::{TableState, TableStateBuilder},
    style::TableStyleBuilder,
    widgets::{Header, Table, TableBuilder, TableEntry, Widget, Width},
};
use ratatui::{
    buffer::Buffer,
    layout::{Margin, Rect},
    widgets::StatefulWidget,
};
use std::fmt::Debug;

pub const COLUMN_COUNT: usize = 11;

/// Resolve a user-supplied column name to its index in [`TableHeader::header`].
/// Matching is case-insensitive and ignores spaces, so `slaveid`, `slave id`, and
/// `Slave ID` all resolve to the same column. Returns `None` if nothing matches.
pub fn column_index(name: &str) -> Option<usize> {
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let target = normalize(name);
    TableHeader::header()
        .iter()
        .position(|h| normalize(h) == target)
}

/// Compare two register rows by the given column for `:order`. Both sides are taken from
/// [`TableEntry::values`] (the displayed strings); numeric when both parse as `f64`,
/// otherwise case-insensitive lexicographic. `descending` reverses the result.
pub fn cmp_definitions(
    a: &Definition,
    b: &Definition,
    column: usize,
    descending: bool,
) -> std::cmp::Ordering {
    let va = a.values();
    let vb = b.values();
    let sa = va.get(column).map(String::as_str).unwrap_or_default();
    let sb = vb.get(column).map(String::as_str).unwrap_or_default();
    let ord = match (sa.parse::<f64>(), sb.parse::<f64>()) {
        (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
        _ => sa.to_lowercase().cmp(&sb.to_lowercase()),
    };
    if descending { ord.reverse() } else { ord }
}

#[derive(Clone, Debug)]
pub struct TableHeader {}

impl Header<COLUMN_COUNT> for TableHeader {
    fn header() -> [String; COLUMN_COUNT] {
        [
            "Name".into(),
            "Description".into(),
            "Slave ID".into(),
            "Address".into(),
            "Access".into(),
            "Kind".into(),
            "Format".into(),
            "Length".into(),
            "Resolution".into(),
            "Value".into(),
            "Raw Value".into(),
        ]
    }

    fn widths() -> [Width; COLUMN_COUNT] {
        [
            Width { min: 15, max: 30 },
            Width { min: 25, max: 40 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 20, max: 30 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 5, max: 20 },
            Width { min: 0, max: 800 },
        ]
    }
}

/// A register row. Metadata comes from `register`; `value`/`raw_value` are filled from a
/// memory snapshot each frame (see `App::refresh_snapshot`).
#[derive(Clone, Debug)]
pub struct Definition {
    pub name: String,
    pub description: String,
    pub register: Register,
    pub named_values: Vec<NamedValue>,
    pub value: String,
    pub raw_value: String,
}

impl Definition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        register: Register,
        named_values: Vec<NamedValue>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            register,
            named_values,
            value: String::new(),
            raw_value: String::new(),
        }
    }
}

impl TableEntry<COLUMN_COUNT> for Definition {
    fn values(&self) -> [String; COLUMN_COUNT] {
        let resolution = match self.register.format().resolution() {
            Some(v) => format!("{}", v),
            None => "None".into(),
        };

        [
            self.name.clone(),
            self.description.clone(),
            format!("{}", self.register.slave_id()),
            format!("{}", self.register.address()),
            format!("{}", self.register.access()),
            format!("{}", self.register.kind()),
            format!("{}", self.register.format()),
            format!("{}", self.register.format().width()),
            resolution,
            self.value.clone(),
            self.raw_value.clone(),
        ]
    }

    fn height(&self) -> u16 {
        3
    }
}

#[focusable]
#[derive(Builder, Focus)]
pub struct TableView {
    #[focus]
    pub table:
        Widget<TableState<Definition, COLUMN_COUNT>, Table<Definition, TableHeader, COLUMN_COUNT>>,
    #[builder(default)]
    pub compact: bool,
}

impl TableView {
    pub fn new(values: Vec<Definition>) -> Self {
        TableViewBuilder::default()
            .table(Widget {
                state: TableStateBuilder::default().values(values).build().unwrap(),
                widget: TableBuilder::default()
                    .style(TableStyleBuilder::default().build().unwrap())
                    .row_margin(Margin {
                        vertical: 1,
                        horizontal: 0,
                    })
                    .build()
                    .unwrap(),
            })
            .focus(TableViewFocus::Table)
            .build()
            .unwrap()
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        StatefulWidget::render(&self.table.widget, area, buf, &mut self.table.state);
    }

    pub fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
        let vertical = if compact { 0 } else { 1 };
        self.table.widget.set_row_margin(Margin {
            vertical,
            horizontal: 0,
        });
    }

    /// Current register rows.
    pub fn definitions(&self) -> &[Definition] {
        self.table.state.values()
    }

    /// Replace the register rows (keeps the table's selection/scroll state).
    pub fn set_definitions(&mut self, values: Vec<Definition>) {
        self.table.state.set_values(values);
    }

    /// Stable-sort the register rows by `column` (see [`cmp_definitions`]).
    pub fn sort_definitions(&mut self, column: usize, descending: bool) {
        let mut values = self.definitions().to_vec();
        values.sort_by(|a, b| cmp_definitions(a, b, column, descending));
        self.set_definitions(values);
    }

    /// The currently selected register row, if any.
    pub fn selected(&self) -> Option<&Definition> {
        let idx = self.table.state.table_state().selected()?;
        self.table.state.values().get(idx)
    }

    /// The index of the currently selected register row, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.table.state.table_state().selected()
    }

    /// Select the first register row (or clear the selection when the table is empty).
    pub fn select_first(&mut self) {
        self.table.state.move_to_top();
    }
}
