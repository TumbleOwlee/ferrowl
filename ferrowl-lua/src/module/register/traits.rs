//! Host-side access traits backing the Lua register module.

use crate::module::register::ValueType;
use mlua::Result;

/// Writes a typed value to the register named `name`.
pub trait Write {
    fn write(&self, name: String, value: ValueType) -> Result<()>;
}

/// Reads the current value of the register named `name`.
pub trait Read {
    fn read(&self, name: String) -> Result<ValueType>;
}

/// Check if the register named `name` exists.
pub trait Has {
    fn has(&self, name: String) -> Result<bool>;
}
