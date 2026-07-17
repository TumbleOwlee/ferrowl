//! Integration tests exercising the host modules (Time, Statics, Register)
//! end-to-end through a real Lua context built with [`ContextBuilder`].

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::time::Duration;

use std::sync::{Arc, Mutex};

use ferrowl_lua::module::{
    Has, LogLevel, LogModule, LogSink, Read, RegisterModule, StaticsModule, TimeModule, ValueType,
    Write,
};
use ferrowl_lua::{Context, ContextBuilder, Error, Result};

/// A host register handle whose reads are keyed by name so each typed getter
/// and each error path can be driven from Lua.
struct Handle;

impl Read for Handle {
    fn read(&self, name: String) -> Result<ValueType> {
        match name.as_str() {
            "i" => Ok(ValueType::Int(7)),
            "f" => Ok(ValueType::Float(1.5)),
            "s" => Ok(ValueType::String("hi".to_string())),
            "b" => Ok(ValueType::Bool(true)),
            // Surfaces the `Err(e)` arm of every getter.
            "err" => Err(Error::RuntimeError("boom".to_string())),
            // Any other name returns an Int, which is the wrong type for the
            // float/string/bool getters -> exercises the type-mismatch arm.
            _ => Ok(ValueType::Int(0)),
        }
    }
}

impl Has for Handle {
    fn has(&self, name: String) -> Result<bool> {
        match name.as_str() {
            "i" | "f" | "s" | "b" => Ok(true),
            // Surfaces the `Err(e)` arm of every getter.
            "err" => Err(Error::RuntimeError("boom".to_string())),
            // Any other name is not present
            _ => Ok(false),
        }
    }
}

impl Write for Handle {
    fn write(&self, _name: String, _value: ValueType) -> Result<()> {
        Ok(())
    }
}

fn statics_module() -> StaticsModule {
    let mut s = StaticsModule::default();
    // Exercises `add` (returns previous value, here None).
    assert!(s.add("i".to_string(), ValueType::Int(42)).is_none());
    s.add("f".to_string(), ValueType::Float(1.5));
    s.add("s".to_string(), ValueType::String("hi".to_string()));
    s.add("b".to_string(), ValueType::Bool(true));
    s
}

const TIME_SCRIPT: &str = r#"
    assert(C_Time:Get() >= 0)
    assert(C_Time:GetMs() >= 0)
"#;

const STATICS_SCRIPT: &str = r#"
    assert(C_Statics:Get("i") == 42)
    assert(C_Statics:Get("f") == 1.5)
    assert(C_Statics:Get("s") == "hi")
    assert(C_Statics:Get("b") == true)
    -- Missing keys error out.
    assert(not pcall(function() return C_Statics:Get("missing") end))
"#;

const REGISTER_SCRIPT: &str = r#"
    assert(C_Register:Get("i") == 7)
    assert(C_Register:Get("f") == 1.5)
    assert(C_Register:Get("s") == "hi")
    assert(C_Register:Get("b") == true)
    -- Set accepts any Lua value type.
    C_Register:Set("x", 99)
    C_Register:Set("s2", "hello")
    C_Register:Set("b2", true)
    -- Host read error propagates.
    assert(not pcall(function() return C_Register:Get("err") end))
"#;

fn build_context() -> Context<String> {
    ContextBuilder::<String>::default()
        .with_stdlib()
        .with_module(TimeModule::default())
        .with_module(statics_module())
        .with_module(RegisterModule::init(Handle))
        .with_script("time".to_string(), TIME_SCRIPT)
        .with_script("statics".to_string(), STATICS_SCRIPT)
        .with_script("register".to_string(), REGISTER_SCRIPT)
        .build()
        .expect("context build failed")
}

#[test]
/// SC-R-027 — a register read returns its natural Lua type and a host read error propagates as a Lua error.
fn ut_modules_run_via_call() {
    let mut ctx = build_context();
    ctx.call(&"time".to_string()).unwrap();
    ctx.call(&"statics".to_string()).unwrap();
    ctx.call(&"register".to_string()).unwrap();
}

#[test]
fn ut_iter_lists_loaded_scripts() {
    let ctx = build_context();
    assert_eq!(ctx.iter().count(), 3);
}

#[test]
fn ut_call_all_ok_when_no_script_errors() {
    let mut ctx = build_context();
    // Every script asserts cleanly, so call_all reports success (the Ok arm).
    assert!(ctx.call_all().is_ok());
}

#[test]
/// SC-R-014 — refresh_all runs each script once per interval, skipping any that ran within the window.
fn ut_refresh_all_runs_then_throttles() {
    let mut ctx = build_context();
    // First pass with a zero window runs everything successfully.
    assert!(ctx.refresh_all(Duration::ZERO).is_ok());
    // A large window skips the just-run scripts (still Ok, nothing executed).
    assert!(ctx.refresh_all(Duration::from_secs(3600)).is_ok());
}

#[test]
/// SC-R-032 — a script error during refresh_all is collected without stopping the pass.
fn ut_refresh_all_collects_errors() {
    let mut ctx = ContextBuilder::<String>::default()
        .with_script("boom".to_string(), "error('x')")
        .build()
        .unwrap();
    let errs = ctx.refresh_all(Duration::ZERO).unwrap_err();
    assert_eq!(errs.len(), 1);
}

#[test]
fn ut_statics_from_constructor() {
    let mut data = HashMap::new();
    data.insert("k".to_string(), ValueType::Int(1));
    let mut ctx = ContextBuilder::<String>::default()
        .with_module(StaticsModule::from(data))
        .with_script("s".to_string(), r#"assert(C_Statics:Get("k") == 1)"#)
        .build()
        .unwrap();
    ctx.call(&"s".to_string()).unwrap();
}

/// A log sink that records printed lines for assertion.
#[derive(Clone, Default)]
struct VecSink(Arc<Mutex<Vec<String>>>);
impl LogSink for VecSink {
    fn log(&self, _level: LogLevel, line: &str) {
        self.0.lock().unwrap().push(line.to_string());
    }
}

#[test]
/// SC-R-031 — C_Log:Info output is routed to the host's module log sink.
fn ut_c_log_info_routes_to_host_sink() {
    let sink = VecSink::default();
    let mut ctx = ContextBuilder::<String>::default()
        .with_stdlib()
        .with_module(LogModule::init(sink.clone()))
        .with_script("log".to_string(), r#"C_Log:Info("hello from lua")"#)
        .build()
        .expect("context build failed");
    ctx.call(&"log".to_string()).unwrap();
    assert_eq!(*sink.0.lock().unwrap(), vec!["hello from lua".to_string()]);
}

#[test]
/// SC-R-033 — a Lua syntax error at load makes context construction fail (all-or-nothing per context).
fn ut_builder_propagates_script_error() {
    // Invalid Lua makes the builder carry the error through to build().
    let result = ContextBuilder::<String>::default()
        .with_script("bad".to_string(), "this is ! not lua")
        .build();
    assert!(result.is_err());
}

#[test]
/// SC-R-001 — scripts run on a real, compiled-in Lua 5.4 VM.
fn ut_runtime_is_lua_5_4() {
    let mut ctx = ContextBuilder::<String>::default()
        .with_script("ver".to_string(), r#"assert(_VERSION == "Lua 5.4")"#)
        .build()
        .unwrap();
    ctx.call(&"ver".to_string()).unwrap();
}

#[test]
/// SC-R-002 — a C_* call is synchronous and blocking: it completes and yields its value before the
/// calling script's next statement runs.
fn ut_host_call_is_synchronous() {
    let mut ctx = ContextBuilder::<String>::default()
        .with_module(TimeModule::default())
        // The second statement uses the value the first call returned: the call must have
        // completed before control returned to the script.
        .with_script(
            "sync".to_string(),
            r#"
            local a = C_Time:GetMs()
            assert(type(a) == "number")
            local b = C_Time:GetMs()
            assert(b >= a)
        "#,
        )
        .build()
        .unwrap();
    ctx.call(&"sync".to_string()).unwrap();
}

#[test]
/// SC-R-004 — every script in one context shares that context's single global environment: a
/// global set by one script is visible to another.
fn ut_scripts_share_one_global_environment() {
    let mut ctx = ContextBuilder::<String>::default()
        .with_script("setter".to_string(), "shared = 123")
        .with_script("getter".to_string(), "assert(shared == 123)")
        .build()
        .unwrap();
    // The setter runs first; the getter then observes the global it left behind.
    ctx.call(&"setter".to_string()).unwrap();
    ctx.call(&"getter".to_string()).unwrap();
}

#[test]
/// SC-R-008 — the only host-injected globals are the C_* modules registered for the context plus
/// `print`; no other bespoke host global is injected.
fn ut_only_registered_host_globals_are_present() {
    let mut ctx = ContextBuilder::<String>::default()
        .with_stdlib()
        .with_module(TimeModule::default())
        .with_print_sink(VecSink::default())
        .with_script(
            "globals".to_string(),
            r#"
            -- Registered for this context:
            assert(C_Time ~= nil)
            assert(print ~= nil)
            -- Not registered here, and no other bespoke host global exists:
            assert(C_Register == nil)
            assert(C_OCPP == nil)
            assert(C_Module == nil)
            assert(C_Anything == nil)
        "#,
        )
        .build()
        .unwrap();
    ctx.call(&"globals".to_string()).unwrap();
}

#[test]
/// SC-R-015 — scripts in a context run sequentially within a cycle: each runs to completion, so a
/// shared global mutated by both reflects both runs regardless of their (unspecified) order.
fn ut_scripts_in_a_cycle_run_sequentially() {
    let mut ctx = ContextBuilder::<String>::default()
        // Order-agnostic: whichever runs first initialises `acc`, the other increments it. If the
        // two could interleave, the read-modify-write would race; sequential execution makes the
        // final value deterministic.
        .with_script("a".to_string(), "acc = (acc or 0) + 1")
        .with_script("b".to_string(), "acc = (acc or 0) + 1")
        .with_script("check".to_string(), "assert(acc == 2)")
        .build()
        .unwrap();
    ctx.call(&"a".to_string()).unwrap();
    ctx.call(&"b".to_string()).unwrap();
    ctx.call(&"check".to_string()).unwrap();
}

#[test]
/// SC-R-019 — a module not registered in the context is a nil global, so naming it fails at run
/// time with an "attempt to index a nil value" error rather than silently no-opping.
fn ut_unregistered_module_indexes_nil_and_errors() {
    // A Modbus-shaped context gets C_Register, never C_OCPP; indexing the absent module errors.
    let mut ctx = ContextBuilder::<String>::default()
        .with_module(RegisterModule::init(Handle))
        .with_script("bad".to_string(), r#"C_OCPP:Get("x")"#)
        .build()
        .unwrap();
    let err = ctx.call(&"bad".to_string()).unwrap_err();
    assert!(
        err.to_string().contains("nil value"),
        "expected a nil-index error, got: {err}"
    );
}
