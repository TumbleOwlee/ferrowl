pub mod traits;

use crate::module::{Module, ValueType};
use mlua::{Result, UserData};
use traits::{Read, Write};

/// Lua module `C_Register`: gives scripts read/write access to named registers
/// through a host-provided [`Read`]/[`Write`] handle.
///
/// Exposed Lua methods: `Get(name)` — returns a number for integer/float
/// registers, a string for strings and a boolean for booleans — and
/// `Set(name, value)`, which accepts any of those Lua types.
pub struct Register<T>
where
    T: Write + Read + 'static,
{
    handle: T,
}

impl<T> Register<T>
where
    T: Write + Read + 'static,
{
    /// Creates the module around the host's register access handle.
    pub fn init(handle: T) -> Self {
        Self { handle }
    }
}

impl<T> Module for Register<T>
where
    T: Write + Read + 'static,
{
    fn module() -> &'static str {
        "C_Register"
    }
}

impl<T> UserData for Register<T>
where
    T: Write + Read + 'static,
{
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Get", Self::get);
        methods.add_method("Set", Self::set);
    }
}

impl<T> Register<T>
where
    T: Write + Read + 'static,
{
    /// `Get(name)` — reads the register and returns its value as the matching
    /// Lua type (number / string / boolean).
    fn get(_: &mlua::Lua, this: &Register<T>, name: String) -> Result<ValueType> {
        this.handle.read(name)
    }

    /// `Set(name, value)` — writes a typed Lua value to the register.
    fn set(_: &mlua::Lua, this: &Register<T>, (name, value): (String, ValueType)) -> Result<()> {
        this.handle.write(name, value)
    }
}
