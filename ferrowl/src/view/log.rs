//! Log pane: scrollable table of timestamped module log lines.

use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{TableState, TableStateBuilder},
    style::TableStyleBuilder,
    widgets::{Table, TableBuilder, Widget},
};
use ferrowl_ui_derive::TableEntry;
use ratatui::layout::Margin;

use crate::app::Level;

/// One log line with a formatted timestamp, severity level and message text.
#[derive(Clone, Debug, TableEntry)]
#[table_entry(header = LogHeader, styles = log_cell_styles)]
pub struct LogEntry {
    #[column(name = "Timestamp", min = 23, max = 23)]
    pub timestamp: String,
    #[column(name = "Level", min = 7, max = 7)]
    pub level: Level,
    #[column(name = "Message", min = 0, max = 100_000)]
    pub message: String,
}

/// Colorize the `Level` column (INFO/WARNING/ERROR); Timestamp and Message stay unstyled.
fn log_cell_styles(row: &LogEntry) -> [Option<ratatui::style::Style>; 3] {
    let level_style = match row.level {
        Level::Info => Some(ratatui::style::Style::default().fg(COLOR_SCHEME.info)),
        Level::Warning => Some(ratatui::style::Style::default().fg(COLOR_SCHEME.warning)),
        Level::Error => Some(ratatui::style::Style::default().fg(COLOR_SCHEME.error)),
    };
    [None, level_style, None]
}

/// The composed log widget: a `Table` plus its scroll/selection state.
pub type LogView = Widget<TableState<LogEntry, 3>, Table<LogEntry, LogHeader, 3>>;

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

/// Derive a per-module log file path from a user-supplied `base` and a module/tab `name`:
/// `<stem>.<sanitized-name>.<ext>` (or `<base>.<name>` without an extension), next to `base`.
/// Mirrors the Modbus module's own path scheme so OCPP `:log` files sit alongside Modbus ones.
pub fn module_log_path(base: &str, name: &str) -> std::path::PathBuf {
    use std::path::{Path, PathBuf};
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = Path::new(base);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ferrowl");
    let filename = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{stem}.{sanitized}.{ext}"),
        None => format!("{stem}.{sanitized}"),
    };
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(filename),
        _ => PathBuf::from(filename),
    }
}

/// Build an empty log view with a border and "Log" title.
pub fn new_log_view() -> LogView {
    Widget {
        state: TableStateBuilder::default()
            .focused(false)
            .values(Vec::new())
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Log".into()))
            .style(
                TableStyleBuilder::default()
                    .focused(
                        ratatui::style::Style::default()
                            .bg(COLOR_SCHEME.hi_bg)
                            .fg(COLOR_SCHEME.text_hi),
                    )
                    .build()
                    .expect("all required builder fields are set"),
            )
            .build()
            .expect("all required builder fields are set"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ui::widgets::{Header, TableEntry};

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

    #[test]
    fn module_log_path_inserts_sanitized_name_before_ext() {
        let p = module_log_path("/tmp/run.log", "cs 1/2");
        assert_eq!(p, std::path::Path::new("/tmp/run.cs_1_2.log"));
        // No extension: append the name.
        let p = module_log_path("/tmp/run", "csms");
        assert_eq!(p, std::path::Path::new("/tmp/run.csms"));
    }

    #[test]
    fn ut_log_entry_and_header_traits() {
        let e = LogEntry {
            timestamp: "ts".into(),
            level: Level::Info,
            message: "hello".into(),
        };
        assert_eq!(
            e.values(),
            ["ts".to_string(), "INFO".to_string(), "hello".to_string()]
        );
        assert_eq!(e.height(), 1);
        assert_eq!(
            LogHeader::header(),
            [
                "Timestamp".to_string(),
                "Level".to_string(),
                "Message".to_string()
            ]
        );
        let w = LogHeader::widths();
        assert_eq!(w[0].min, 23);
        assert_eq!(w[0].max, 23);
    }

    #[test]
    fn ut_log_cell_styles_colorizes_level_only() {
        let e = LogEntry {
            timestamp: "ts".into(),
            level: Level::Warning,
            message: "hello".into(),
        };
        let styles = e.cell_styles();
        assert!(styles[0].is_none());
        assert_eq!(
            styles[1],
            Some(ratatui::style::Style::default().fg(COLOR_SCHEME.warning))
        );
        assert!(styles[2].is_none());
    }

    #[test]
    fn ut_new_log_view_is_empty_and_unfocused() {
        let view = new_log_view();
        assert!(!view.state.focused());
        assert!(view.state.values().is_empty());
    }
}
