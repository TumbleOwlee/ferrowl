//! Lua module `C_OCPP`: gives scripts access to an OCPP charging-station's live state via
//! `Get(name)`/`Set(name, value)` (backed by the host [`Read`]/[`Write`] handle) plus one method
//! per supported action — `C_OCPP:<Action>(overrides?)` — which enqueues the action through the
//! host [`OcppActions`] handle and returns a boolean (`false` on error).
//!
//! The exposed action set is version-specific (OCPP 1.6 vs 2.0.1) and comes from the host handle's
//! [`OcppActions::actions`], so two host handles produce two logically distinct `C_OCPP` modules.

pub mod traits;

use crate::module::{Module, Read, ValueType, Write};
use mlua::{Result, Table, UserData};
use traits::OcppActions;

/// Lua module `C_OCPP`, parameterised over a host handle providing state read/write and action
/// dispatch.
pub struct Ocpp<H>
where
    H: Read + Write + OcppActions + 'static,
{
    handle: H,
}

impl<H> Ocpp<H>
where
    H: Read + Write + OcppActions + 'static,
{
    /// Creates the module around the host handle.
    pub fn init(handle: H) -> Self {
        Self { handle }
    }
}

impl<H> Module for Ocpp<H>
where
    H: Read + Write + OcppActions + 'static,
{
    fn module() -> &'static str {
        "C_OCPP"
    }
}

impl<H> UserData for Ocpp<H>
where
    H: Read + Write + OcppActions + 'static,
{
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // `Get(name)` / `Set(name, value)` mirror the register module.
        methods.add_method("Get", |_, this, name: String| this.handle.read(name));
        methods.add_method("Set", |_, this, (name, value): (String, ValueType)| {
            this.handle.write(name, value)
        });

        // One method per version-specific action: `C_OCPP:<Action>(overrides?)`.
        for action in H::actions() {
            methods.add_method(action, move |_, this, args: Option<Table>| {
                match table_to_overrides(args) {
                    Ok(overrides) => Ok(this.handle.dispatch(action, overrides)),
                    // Bad argument shape (e.g. a non-scalar override value) -> false, not a raise.
                    Err(_) => Ok(false),
                }
            });
        }
    }
}

/// Flatten an optional Lua override table into `(key, value)` scalar pairs. A missing table is no
/// overrides; a non-string key or non-scalar value surfaces as an error (handled as `false`).
fn table_to_overrides(table: Option<Table>) -> Result<Vec<(String, ValueType)>> {
    let mut out = Vec::new();
    if let Some(table) = table {
        for pair in table.pairs::<String, ValueType>() {
            out.push(pair?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBuilder;
    use crate::module::{Read, Write};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    /// A mock host: an in-memory key/value store plus a record of dispatched actions.
    #[derive(Clone, Default)]
    struct MockHandle {
        store: Rc<RefCell<HashMap<String, ValueType>>>,
        dispatched: Rc<RefCell<Vec<(String, Vec<(String, ValueType)>)>>>,
    }

    impl Read for MockHandle {
        fn read(&self, name: String) -> mlua::Result<ValueType> {
            self.store
                .borrow()
                .get(&name)
                .cloned()
                .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown '{name}'")))
        }
    }
    impl Write for MockHandle {
        fn write(&self, name: String, value: ValueType) -> mlua::Result<()> {
            self.store.borrow_mut().insert(name, value);
            Ok(())
        }
    }
    impl OcppActions for MockHandle {
        fn actions() -> Vec<&'static str> {
            vec!["StartTransaction"]
        }
        fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
            self.dispatched.borrow_mut().push((action.to_string(), args));
            true
        }
    }

    #[test]
    fn ut_get_set_roundtrip_and_dispatch() {
        let handle = MockHandle::default();
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(Ocpp::init(handle.clone()))
            .with_script(
                "s".to_string(),
                r#"
                C_OCPP:Set("Power", 42)
                C_OCPP:Set("Power", C_OCPP:Get("Power") + 1)
                ok = C_OCPP:StartTransaction({ idTag = "ABC" })
                bad = C_OCPP.Get == nil
                "#,
            )
            .build()
            .expect("build context");

        ctx.call_all(std::time::Duration::ZERO).expect("run");

        // Set/Get round-tripped through the host store.
        match handle.store.borrow().get("Power") {
            Some(ValueType::Int(v)) => assert_eq!(*v, 43),
            other => panic!("expected Int(43), got {other:?}"),
        }
        // The action was enqueued with its override arg.
        let dispatched = handle.dispatched.borrow();
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].0, "StartTransaction");
        assert_eq!(dispatched[0].1.len(), 1);
        assert_eq!(dispatched[0].1[0].0, "idTag");
    }
}
