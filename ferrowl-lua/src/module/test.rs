//! Lua module `C_Test`: assertion helpers for test/CI scripts.
//!
//! `Assert(cond, msg)` raises a Lua error when `cond` is falsy (Lua truthiness: only `nil` and
//! `false` are falsy). `Fail(msg)` always raises. Both surface as `"assertion failed: {msg}"` so
//! a headless runner watching the module log can key off that text.

use ferrowl_lua_derive::Module;
use mlua::{Error, Result, UserData, Value};

/// Lua module `C_Test`: exposes `Assert(cond, msg)` and `Fail(msg)`.
#[derive(Module, Default)]
#[module = "C_Test"]
pub struct Test;

impl UserData for Test {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Assert", |_, _, (cond, msg): (Value, String)| {
            if is_falsy(&cond) {
                Err(assertion_failed(&msg))
            } else {
                Ok(())
            }
        });
        methods.add_method("Fail", |_, _, msg: String| -> Result<()> {
            Err(assertion_failed(&msg))
        });
    }
}

/// Lua truthiness: everything but `nil` and `false` is truthy.
fn is_falsy(v: &Value) -> bool {
    matches!(v, Value::Nil) || matches!(v, Value::Boolean(false))
}

fn assertion_failed(msg: &str) -> Error {
    Error::RuntimeError(format!("assertion failed: {msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;
    use mlua::Lua;

    fn lua_with_test() -> Lua {
        let lua = Lua::new();
        lua.globals().set("C_Test", Test).unwrap();
        lua
    }

    #[test]
    fn ut_module_name() {
        assert_eq!(<Test as Module>::module(), "C_Test");
    }

    #[test]
    fn ut_assert_true_passes() {
        let lua = lua_with_test();
        lua.load(r#"C_Test:Assert(true, "should not fire")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn ut_assert_truthy_value_passes() {
        let lua = lua_with_test();
        lua.load(r#"C_Test:Assert(1, "should not fire")"#)
            .exec()
            .unwrap();
        lua.load(r#"C_Test:Assert("x", "should not fire")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn ut_assert_false_raises() {
        let lua = lua_with_test();
        let err = lua
            .load(r#"C_Test:Assert(false, "boom")"#)
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("assertion failed: boom"));
    }

    #[test]
    fn ut_assert_nil_raises() {
        let lua = lua_with_test();
        let err = lua
            .load(r#"C_Test:Assert(nil, "was nil")"#)
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("assertion failed: was nil"));
    }

    #[test]
    fn ut_fail_always_raises() {
        let lua = lua_with_test();
        let err = lua.load(r#"C_Test:Fail("nope")"#).exec().unwrap_err();
        assert!(err.to_string().contains("assertion failed: nope"));
    }
}
