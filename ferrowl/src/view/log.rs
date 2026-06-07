use ferrowl_ui::{
    COLOR_SCHEME,
    state::{TableState, TableStateBuilder},
    style::TableStyleBuilder,
    types::Border,
    widgets::{Header, Table, TableBuilder, TableEntry, Widget, Width},
};
use ratatui::layout::Margin;

/// One log line with a formatted timestamp and message text.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub timestamp: String,
    pub message: String,
}

impl TableEntry<2> for LogEntry {
    fn values(&self) -> [String; 2] {
        [self.timestamp.clone(), self.message.clone()]
    }

    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone, Debug)]
pub struct LogHeader;

impl Header<2> for LogHeader {
    fn header() -> [String; 2] {
        ["Timestamp".to_string(), "Message".to_string()]
    }

    fn widths() -> [Width; 2] {
        [
            Width { min: 23, max: 23 },
            Width { min: 0, max: 100_000 },
        ]
    }
}

/// The composed log widget: a `Table` plus its scroll/selection state.
pub type LogView = Widget<TableState<LogEntry, 2>, Table<LogEntry, LogHeader, 2>>;

/// Format a Unix-millisecond timestamp as `YYYY-MM-DD HH:MM:SS.mmm` (UTC).
pub fn format_timestamp(ms: u64) -> String {
    let total_secs = ms / 1000;
    let millis = ms % 1000;
    let h = (total_secs / 3600) % 24;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    let (year, month, day) = civil_from_days((total_secs / 86400) as i64);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Convert days since the Unix epoch (1970-01-01) to a Gregorian (year, month, day) triple.
/// Uses the algorithm by Howard Hinnant.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era: i64 = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00.000");
    }

    #[test]
    fn known_date_with_millis() {
        // 2026-06-07 12:34:56.789 UTC
        // days = 20611, time = 12*3600 + 34*60 + 56 = 45296 s, ms = 789
        let ms: u64 = 20611 * 86400 * 1000 + 45296 * 1000 + 789;
        assert_eq!(format_timestamp(ms), "2026-06-07 12:34:56.789");
    }

    #[test]
    fn leap_day() {
        // 2024-02-29 is day 19782 since epoch
        let ms: u64 = 19782 * 86400 * 1000;
        assert_eq!(format_timestamp(ms), "2024-02-29 00:00:00.000");
    }
}

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
