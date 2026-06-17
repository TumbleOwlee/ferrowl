//! Integration tests exercising the host modules (Time, Statics, Register)
//! end-to-end through a real Lua context built with [`ContextBuilder`].

use std::collections::HashMap;
use std::time::Duration;

use ferrowl_lua::module::{Read, RegisterModule, StaticsModule, TimeModule, ValueType, Write};
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
    assert!(ctx.call_all(Duration::ZERO).is_ok());
}

#[test]
fn ut_refresh_all_runs_then_throttles() {
    let mut ctx = build_context();
    // First pass with a zero window runs everything successfully.
    assert!(ctx.refresh_all(Duration::ZERO).is_ok());
    // A large window skips the just-run scripts (still Ok, nothing executed).
    assert!(ctx.refresh_all(Duration::from_secs(3600)).is_ok());
}

#[test]
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

#[test]
fn ut_builder_propagates_script_error() {
    // Invalid Lua makes the builder carry the error through to build().
    let result = ContextBuilder::<String>::default()
        .with_script("bad".to_string(), "this is ! not lua")
        .build();
    assert!(result.is_err());
}
