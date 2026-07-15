//! Integration tests for the `print` override: stdlib's global `print` writes to stdout, which
//! would corrupt the host TUI's alternate screen, so `with_print_sink` redirects it to a host
//! `LogSink` instead.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use ferrowl_lua::ContextBuilder;
use ferrowl_lua::module::{LogLevel, LogSink};

/// A log sink that records printed lines for assertion.
#[derive(Clone, Default)]
struct VecSink(Arc<Mutex<Vec<String>>>);
impl LogSink for VecSink {
    fn log(&self, _level: LogLevel, line: &str) {
        self.0.lock().unwrap().push(line.to_string());
    }
}

#[test]
fn ut_print_joins_args_with_tabs() {
    let sink = VecSink::default();
    let mut ctx = ContextBuilder::<String>::default()
        .with_stdlib()
        .with_print_sink(sink.clone())
        .with_script("p".to_string(), r#"print("a", 1, true)"#)
        .build()
        .expect("context build failed");
    ctx.call(&"p".to_string()).unwrap();
    assert_eq!(*sink.0.lock().unwrap(), vec!["a\t1\ttrue".to_string()]);
}

#[test]
fn ut_print_no_args_is_empty_line() {
    let sink = VecSink::default();
    let mut ctx = ContextBuilder::<String>::default()
        .with_stdlib()
        .with_print_sink(sink.clone())
        .with_script("p".to_string(), "print()")
        .build()
        .expect("context build failed");
    ctx.call(&"p".to_string()).unwrap();
    assert_eq!(*sink.0.lock().unwrap(), vec!["".to_string()]);
}

#[test]
fn ut_print_honors_tostring_metamethod() {
    let sink = VecSink::default();
    let mut ctx = ContextBuilder::<String>::default()
        .with_stdlib()
        .with_print_sink(sink.clone())
        .with_script(
            "p".to_string(),
            r#"
                local t = setmetatable({}, { __tostring = function() return "custom" end })
                print(t)
            "#,
        )
        .build()
        .expect("context build failed");
    ctx.call(&"p".to_string()).unwrap();
    assert_eq!(*sink.0.lock().unwrap(), vec!["custom".to_string()]);
}

#[test]
fn ut_with_print_sink_before_stdlib_still_redirects() {
    // Empirically, `enable_stdlib`'s `load_std_libs(ALL_SAFE)` does not reload `base` (`print` is
    // untouched), so the override survives regardless of call order. Verified here so a future
    // mlua upgrade that changes this behavior gets caught.
    let sink = VecSink::default();
    let mut ctx = ContextBuilder::<String>::default()
        .with_print_sink(sink.clone())
        .with_stdlib()
        .with_script("p".to_string(), r#"print("still redirected")"#)
        .build()
        .expect("context build failed");
    ctx.call(&"p".to_string()).unwrap();
    assert_eq!(
        *sink.0.lock().unwrap(),
        vec!["still redirected".to_string()]
    );
}
