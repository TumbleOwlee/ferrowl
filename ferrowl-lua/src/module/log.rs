//! Lua module `C_Log`: print a line to the host's module log.
//!
//! The host (a ferrowl module) supplies a [`LogSink`] that routes a printed line wherever its log
//! lives (e.g. the on-screen ring log + the optional file sink). Keeping the sink behind a trait
//! lets this crate stay agnostic of the host's logging types.

use ferrowl_lua_derive::Module;
use mlua::UserData;

/// Severity of a line logged from Lua. Mirrors the host's own level type; kept local to this
/// crate since `ferrowl-lua` cannot depend on `ferrowl` (the dependency runs the other way) —
/// implementors of [`LogSink`] translate this into their own level type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

/// A host-provided sink for lines logged from Lua via `C_Log:Info(..)`/`Warn(..)`/`Error(..)`.
pub trait LogSink {
    /// Append one line to the host's log. Called from the (non-async) script thread.
    fn log(&self, level: LogLevel, line: &str);
}

/// Lua module `C_Log`: exposes `Info`/`Warn`/`Error` methods which append a line to the host log.
#[derive(Module)]
#[module = "C_Log"]
pub struct Log<S: LogSink> {
    sink: S,
}

impl<S: LogSink> Log<S> {
    /// Build the module over a host log sink.
    pub fn init(sink: S) -> Self {
        Self { sink }
    }
}

impl<S: LogSink + 'static> UserData for Log<S> {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Info", |_, this, line: String| {
            this.sink.log(LogLevel::Info, &line);
            Ok(())
        });
        methods.add_method("Warn", |_, this, line: String| {
            this.sink.log(LogLevel::Warning, &line);
            Ok(())
        });
        methods.add_method("Error", |_, this, line: String| {
            this.sink.log(LogLevel::Error, &line);
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct VecSink(Arc<Mutex<Vec<(LogLevel, String)>>>);
    impl LogSink for VecSink {
        fn log(&self, level: LogLevel, line: &str) {
            self.0.lock().unwrap().push((level, line.to_string()));
        }
    }

    #[test]
    /// SC-R-031 — a line logged via C_Log is routed to the host's module log sink.
    fn ut_log_routes_to_sink() {
        let sink = VecSink::default();
        let log = Log::init(sink.clone());
        log.sink.log(LogLevel::Info, "hello");
        assert_eq!(
            *sink.0.lock().unwrap(),
            vec![(LogLevel::Info, "hello".to_string())]
        );
        assert_eq!(<Log<VecSink> as Module>::module(), "C_Log");
    }
}
