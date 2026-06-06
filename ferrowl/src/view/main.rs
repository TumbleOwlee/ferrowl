use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_reg::Register;
use crate::config::device::NamedValue;
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{TableState, TableStateBuilder},
    style::TableStyleBuilder,
    widgets::{Header, Table, TableBuilder, TableEntry, Widget, Width},
};
use ratatui::{
    buffer::Buffer,
    layout::{Margin, Rect},
    style::palette::tailwind,
    widgets::StatefulWidget,
};
use std::fmt::Debug;

pub const COLUMN_COUNT: usize = 11;

#[derive(Clone, Debug)]
pub struct TableHeader {}

impl Header<COLUMN_COUNT> for TableHeader {
    fn header() -> [String; COLUMN_COUNT] {
        [
            "Name".into(),
            "Comment".into(),
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
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
            Width { min: 10, max: 20 },
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
    pub comment: String,
    pub register: Register,
    pub named_values: Vec<NamedValue>,
    pub value: String,
    pub raw_value: String,
}

impl Definition {
    pub fn new(
        name: impl Into<String>,
        comment: impl Into<String>,
        register: Register,
        named_values: Vec<NamedValue>,
    ) -> Self {
        Self {
            name: name.into(),
            comment: comment.into(),
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
            self.comment.clone(),
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
}

impl TableView {
    pub fn new(values: Vec<Definition>) -> Self {
        TableViewBuilder::default()
            .table(Widget {
                state: TableStateBuilder::default().values(values).build().unwrap(),
                widget: TableBuilder::default()
                    .style(
                        TableStyleBuilder::default()
                            .focused(
                                ratatui::style::Style::default()
                                    .bg(tailwind::INDIGO.c900)
                                    .fg(COLOR_SCHEME.text),
                            )
                            .build()
                            .unwrap(),
                    )
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

    /// Current register rows.
    pub fn definitions(&self) -> &[Definition] {
        self.table.state.values()
    }

    /// Replace the register rows (keeps the table's selection/scroll state).
    pub fn set_definitions(&mut self, values: Vec<Definition>) {
        self.table.state.set_values(values);
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
}
