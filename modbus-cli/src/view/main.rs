use derive_builder::Builder;
use modbus_derive::{Focus, focusable};
use modbus_mem::{Memory, Range};
use modbus_net::SlaveId;
use modbus_reg::Register;
use modbus_ui::{
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
use std::{fmt::Debug, sync::Arc};

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

#[derive(Clone, Builder)]
pub struct Definition {
    name: String,
    comment: String,
    register: Register,
    memory: Arc<Memory<SlaveId>>,
}

impl TableEntry<COLUMN_COUNT> for Definition {
    fn values(&self) -> [String; COLUMN_COUNT] {
        let resolution = if let Some(v) = self.register.format().resolution() {
            format!("{}", v)
        } else {
            "None".into()
        };

        let (value, raw_value) = match self.register.address() {
            modbus_reg::Address::Virtual => (String::new(), String::new()),
            modbus_reg::Address::Fixed(addr) => {
                let range = Range::new(*addr as usize, self.register.format().width());
                let raw_value =
                    self.memory
                        .read(
                            *self.register.slave_id(),
                            match self.register.kind() {
                                modbus_reg::Kind::Coil | modbus_reg::Kind::DiscreteInput => {
                                    &modbus_mem::Type::Coil
                                }
                                modbus_reg::Kind::InputRegister
                                | modbus_reg::Kind::HoldingRegister => &modbus_mem::Type::Register,
                            },
                            &range,
                        )
                        .unwrap_or(vec![0; self.register.format().width()]);

                let value = self
                    .register
                    .decode(&raw_value)
                    .unwrap_or(modbus_reg::value::Value::Ascii("Error".to_string()));
                (format!("{}", value), format!("{:?}", raw_value))
            }
        };

        [
            /*Name*/ self.name.clone(),
            /*Comment*/ self.comment.clone(),
            /*Slave ID*/ format!("{}", self.register.slave_id()),
            /*Address*/ format!("{}", self.register.address()),
            /*Access*/ format!("{}", self.register.access()),
            /*Kind*/ format!("{}", self.register.kind()),
            /*Format*/ format!("{}", self.register.format()),
            /*Length*/ format!("{}", self.register.format().width()),
            /*Resolution*/ resolution,
            /*Value*/ value,
            /*Raw Value*/ raw_value,
        ]
    }
    fn height(&self) -> u16 {
        3
    }
}

#[focusable]
#[derive(Builder, Focus)]
pub struct TableView {
    // Label for the register
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
                                    .fg(tailwind::SLATE.c200),
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
}
