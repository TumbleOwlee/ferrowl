//! Per-module file-sink plumbing: the optional log file a running module can be pointed at
//! (`:log <file>`), independent of the in-memory ring log.

use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Append a timestamped line to the file sink (if any), flushing so it's durable.
pub(crate) fn append(sink: &FileSink, line: &str) {
    if let Ok(mut guard) = sink.lock()
        && let Some(writer) = guard.as_mut()
    {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let ts = format_timestamp(ms);
        let _ = writeln!(writer, "[{ts}] {line}");
        let _ = writer.flush();
    }
}

#[cfg(test)]
mod tests {
    #[test]
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
    fn ut_open_sink_error_on_nonexistent_dir() {
        use super::{FileSink, open_sink};

        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let result = open_sink(&sink, Some("/nonexistent/dir/base.log"), "test");
        assert!(result.is_err());
        // Verify sink was cleared on error.
        let guard = sink.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    fn ut_open_sink_success_with_valid_dir() {
        use super::{FileSink, open_sink};

        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let temp_dir = std::env::temp_dir();
        let base = temp_dir
            .join("ferrowl_test.log")
            .to_string_lossy()
            .into_owned();
        let result = open_sink(&sink, Some(&base), "test");
        assert!(result.is_ok());
        // Verify sink has a writer.
        let guard = sink.lock().unwrap();
        assert!(guard.is_some());
        drop(guard);
        // Cleanup.
        let _ = std::fs::remove_file(temp_dir.join("ferrowl_test.test.log"));
    }

    #[test]
    fn ut_open_sink_clears_on_none_base() {
        use super::{FileSink, open_sink};

        let sink: FileSink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let result = open_sink(&sink, None, "test");
        assert!(result.is_ok());
        // Verify sink remains None.
        let guard = sink.lock().unwrap();
        assert!(guard.is_none());
    }
}
