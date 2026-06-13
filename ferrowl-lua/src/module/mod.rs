//! Host modules exposed to Lua scripts as global userdata objects.

mod register;
mod statics;
mod time;

pub use register::Register as RegisterModule;
pub use register::traits::{Read, Write};
pub use statics::Statics as StaticsModule;
pub use time::Time as TimeModule;

/// A dynamically typed value passed between Lua and the host.
#[derive(Clone)]
pub enum ValueType {
    Int(i128),
    Float(f64),
    String(String),
    Bool(bool),
}

impl mlua::IntoLua for ValueType {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
        match self {
            ValueType::Int(v) => v.into_lua(lua),
            ValueType::Float(v) => v.into_lua(lua),
            ValueType::String(v) => v.into_lua(lua),
            ValueType::Bool(v) => v.into_lua(lua),
        }
    }
}

impl mlua::FromLua for ValueType {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        match value {
            mlua::Value::Integer(v) => Ok(ValueType::Int(v as i128)),
            mlua::Value::Number(v) => Ok(ValueType::Float(v)),
            mlua::Value::String(v) => Ok(ValueType::String(v.to_str()?.to_owned())),
            mlua::Value::Boolean(v) => Ok(ValueType::Bool(v)),
            other => Err(mlua::Error::FromLuaConversionError {
                from: other.type_name(),
                to: "ValueType".to_string(),
                message: Some("expected number, string or boolean".to_string()),
            }),
        }
    }
}

/// A host module that can be registered in a Lua context.
pub trait Module {
    /// The Lua global name the module is registered under (e.g. `C_Time`).
    fn module() -> &'static str;
}
