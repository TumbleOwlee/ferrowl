use std::collections::HashMap;

use crate::module::{Module, ValueType};
use mlua::{Result, UserData};

/// Lua module `C_Statics`: read-only key/value store of host-provided
/// constants.
///
/// Exposed Lua methods: `GetInt`, `GetFloat`, `GetString`, `GetBool` — each
/// takes a key and errors if the key is missing or holds a different type.
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
        methods.add_method("GetInt", Self::get_int);
        methods.add_method("GetFloat", Self::get_float);
        methods.add_method("GetString", Self::get_string);
        methods.add_method("GetBool", Self::get_bool);
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

    fn get_int(_: &mlua::Lua, this: &Statics, name: String) -> Result<i128> {
        if let Some(ValueType::Int(i)) = this.data.get(&name) {
            Ok(*i)
        } else {
            Err(mlua::Error::UserDataTypeMismatch)
        }
    }

    fn get_float(_: &mlua::Lua, this: &Statics, name: String) -> Result<f64> {
        if let Some(ValueType::Float(f)) = this.data.get(&name) {
            Ok(*f)
        } else {
            Err(mlua::Error::UserDataTypeMismatch)
        }
    }

    fn get_string(_: &mlua::Lua, this: &Statics, name: String) -> Result<String> {
        if let Some(ValueType::String(s)) = this.data.get(&name) {
            Ok(s.to_owned())
        } else {
            Err(mlua::Error::UserDataTypeMismatch)
        }
    }

    fn get_bool(_: &mlua::Lua, this: &Statics, name: String) -> Result<bool> {
        if let Some(ValueType::Bool(b)) = this.data.get(&name) {
            Ok(*b)
        } else {
            Err(mlua::Error::UserDataTypeMismatch)
        }
    }
}
