use std::collections::HashMap;

use crate::module::{Module, ValueType};
use mlua::{Result, UserData};

/// Lua module `C_Statics`: read-only key/value store of host-provided
/// constants.
///
/// Exposed Lua method: `Get(name)` — returns the stored constant as the
/// matching Lua type (number / string / boolean), erroring if the key is
/// missing.
#[allow(dead_code)]
#[derive(Default)]
pub struct Statics {
    data: HashMap<String, ValueType>,
}

impl Module for Statics {
    fn module() -> &'static str {
        "C_Statics"
    }
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
