//! Scrollable log pane, rendered as a single-column [`Table`] (the user's "table view
//! for logging messages"). Lines are fed from a module's ring [`ferrowl_log::Log`] each
//! frame; the underlying `TableState` provides vertical scrolling + a scrollbar.

use ferrowl_ui::{
    COLOR_SCHEME,
    state::{TableState, TableStateBuilder},
    style::TableStyleBuilder,
    types::Border,
    widgets::{Header, Table, TableBuilder, TableEntry, Widget, Width},
};
use ratatui::layout::Margin;

/// One log line.
#[derive(Clone, Debug)]
pub struct LogEntry(pub String);

impl TableEntry<1> for LogEntry {
    fn values(&self) -> [String; 1] {
        [self.0.clone()]
    }

    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone, Debug)]
pub struct LogHeader;

impl Header<1> for LogHeader {
    fn header() -> [String; 1] {
        ["Message".to_string()]
    }

    fn widths() -> [Width; 1] {
        [Width {
            min: 0,
            max: 100_000,
        }]
    }
}

/// The composed log widget: a `Table` plus its scroll/selection state.
pub type LogView = Widget<TableState<LogEntry, 1>, Table<LogEntry, LogHeader, 1>>;

/// Build an empty log view with a border and "Log" title.
pub fn new_log_view() -> LogView {
    Widget {
        state: TableStateBuilder::default()
            .focused(false)
            .values(Vec::new())
            .build()
            .unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Log".into()))
            .style(
                TableStyleBuilder::default()
                    .focused(
                        ratatui::style::Style::default()
                            .bg(COLOR_SCHEME.hi_bg)
                            .fg(COLOR_SCHEME.text),
                    )
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    }
}
