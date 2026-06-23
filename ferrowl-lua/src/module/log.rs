//! Lua module `C_Log`: print a line to the host's module log.
//!
//! The host (a ferrowl module) supplies a [`LogSink`] that routes a printed line wherever its log
//! lives (e.g. the on-screen ring log + the optional file sink). Keeping the sink behind a trait
//! lets this crate stay agnostic of the host's logging types.

use ferrowl_lua_derive::Module;
use mlua::UserData;

/// A host-provided sink for lines printed from Lua via `C_Log:Print(..)`.
pub trait LogSink {
    /// Append one line to the host's log. Called from the (non-async) script thread.
    fn print(&self, line: &str);
}

/// Lua module `C_Log`: exposes `Print(message)` which appends a line to the host log.
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
        methods.add_method("Print", |_, this, line: String| {
            this.sink.print(&line);
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
    struct VecSink(Arc<Mutex<Vec<String>>>);
    impl LogSink for VecSink {
        fn print(&self, line: &str) {
            self.0.lock().unwrap().push(line.to_string());
        }
    }

    #[test]
    fn ut_print_routes_to_sink() {
        let sink = VecSink::default();
        let log = Log::init(sink.clone());
        log.sink.print("hello");
        assert_eq!(*sink.0.lock().unwrap(), vec!["hello".to_string()]);
        assert_eq!(<Log<VecSink> as Module>::module(), "C_Log");
    }
}
