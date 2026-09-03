//! Per-module file-sink plumbing: the optional log file a running module can be pointed at
//! (`:log <file>`), independent of the in-memory ring log.

use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::view::log::format_timestamp;

/// Optional per-module log file, shared into the log/status callbacks; swappable at runtime so
/// `:log` takes effect on already-running modules.
pub(crate) type FileSink = Arc<Mutex<Option<BufWriter<std::fs::File>>>>;

/// Open (append) the per-module log file for `base`, or clear the sink when `base` is `None`.
/// Returns an error if the file can't be opened (in which case the sink is cleared).
pub(crate) fn open_sink(
    sink: &FileSink,
    base: Option<&str>,
    name: &str,
) -> Result<(), std::io::Error> {
    if let Some(base) = base {
        let path = module_log_path(base, name);
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                if let Ok(mut guard) = sink.lock() {
                    *guard = Some(BufWriter::new(file));
                }
                Ok(())
            }
            Err(e) => {
                if let Ok(mut guard) = sink.lock() {
                    *guard = None;
                }
                Err(e)
            }
        }
    } else {
        if let Ok(mut guard) = sink.lock() {
            *guard = None;
        }
        Ok(())
    }
}

/// `<stem>.<sanitized-name>.<ext>` (or `<base>.<name>` without an extension), next to `base`.
fn module_log_path(base: &str, name: &str) -> PathBuf {
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
    let base = ferrowl_util::path::expand(base);
    let path = base.as_path();
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

/// Append a timestamped line to the file sink (if any), flushing so it's durable.
pub(crate) fn append(sink: &FileSink, line: &str) {
    if let Ok(mut guard) = sink.lock()
        && let Some(writer) = guard.as_mut()
    {
        let ms = ferrowl_util::time::now_unix_ms();
        let ts = format_timestamp(ms);
        let _ = writeln!(writer, "[{ts}] {line}");
        let _ = writer.flush();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    /// UI-R-085 — the module file-sink path is derived from the base path and module name.
    fn ut_module_log_path() {
        use super::module_log_path;
        assert_eq!(
            module_log_path("ferrowl.log", "evse-1"),
            std::path::PathBuf::from("ferrowl.evse-1.log")
        );
        assert_eq!(
            module_log_path("logs/run.log", "evse 1"),
            std::path::PathBuf::from("logs/run.evse_1.log")
        );
        assert_eq!(
            module_log_path("out", "m"),
            std::path::PathBuf::from("out.m")
        );
    }

    #[test]
    /// NF-R-054 — `module_log_path` expands a leading `~` in `base` to the home directory.
    fn ut_module_log_path_expands_tilde() {
        use super::module_log_path;
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        assert_eq!(
            module_log_path("~/run.log", "cs 1"),
            home.join("run.cs_1.log")
        );
    }

    #[test]
    /// NF-R-054 — `open_sink` resolves a `~`-prefixed base against the real home directory.
    fn ut_open_sink_expands_tilde() {
        use super::{FileSink, open_sink};
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let result = open_sink(&sink, Some("~/ferrowl_nfr042_open_sink_test.log"), "test");
        assert!(result.is_ok());
        assert!(sink.lock().unwrap().is_some());
        let _ = std::fs::remove_file(home.join("ferrowl_nfr042_open_sink_test.test.log"));
    }

    #[test]
    /// UI-R-085 — opening a file sink in a nonexistent directory errors.
    fn ut_open_sink_error_on_nonexistent_dir() {
        use super::{FileSink, open_sink};

        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let result = open_sink(&sink, Some("/nonexistent/dir/base.log"), "test");
        assert!(result.is_err());
        let guard = sink.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    /// UI-R-085 — a file sink opens against a valid directory.
    fn ut_open_sink_success_with_valid_dir() {
        use super::{FileSink, open_sink};
        use ferrowl_test_support::reserve_temp_dir;

        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let temp_dir = reserve_temp_dir("ferrowl_modbus_log");
        let base = temp_dir.join("test.log").to_string_lossy().into_owned();
        let result = open_sink(&sink, Some(&base), "test");
        assert!(result.is_ok());
        let guard = sink.lock().unwrap();
        assert!(guard.is_some());
        drop(guard);
    }

    #[test]
    /// UI-R-085 — a None base path clears the file sink.
    fn ut_open_sink_clears_on_none_base() {
        use super::{FileSink, open_sink};

        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let result = open_sink(&sink, None, "test");
        assert!(result.is_ok());
        let guard = sink.lock().unwrap();
        assert!(guard.is_none());
    }
}
