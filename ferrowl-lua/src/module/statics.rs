use std::collections::HashMap;

use crate::module::ValueType;
use ferrowl_lua_derive::Module;
use mlua::{Result, UserData};

/// Lua module `C_Statics`: read-only key/value store of host-provided
/// constants.
///
/// Exposed Lua method: `Get(name)` — returns the stored constant as the
/// matching Lua type (number / string / boolean), erroring if the key is
/// missing.
#[derive(Default, Module)]
#[module = "C_Statics"]
pub struct Statics {
    data: HashMap<String, ValueType>,
}

impl UserData for Statics {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Get", Self::get);
    }
}

impl Statics {
    /// Creates the module pre-populated with `data`.
    pub fn from(data: HashMap<String, ValueType>) -> Self {
        Self { data }
    }

    /// Inserts a value, returning the previous value stored under `key`.
    pub fn add(&mut self, key: String, value: ValueType) -> Option<ValueType> {
        self.data.insert(key, value)
    }

    /// `Get(name)` — returns the stored constant as the matching Lua type,
    /// erroring if the key is unknown.
    fn get(_: &mlua::Lua, this: &Statics, name: String) -> Result<ValueType> {
        this.data
            .get(&name)
            .cloned()
            .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown static '{name}'")))
    }
}
