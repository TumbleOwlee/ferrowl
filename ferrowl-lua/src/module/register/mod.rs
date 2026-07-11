pub mod traits;

use crate::module::ValueType;
use ferrowl_lua_derive::Module;
use mlua::{Result, UserData};
use traits::{Has, Read, Write};

/// Lua module `C_Register`: gives scripts read/write access to named registers
/// through a host-provided [`Read`]/[`Write`] handle.
///
/// Exposed Lua methods: `Get(name)` — returns a number for integer/float
/// registers, a string for strings and a boolean for booleans — and
/// `Set(name, value)`, which accepts any of those Lua types.
#[derive(Module)]
#[module = "C_Register"]
pub struct Register<T>
where
    T: Write + Read + Has + 'static,
{
    handle: T,
}

impl<T> Register<T>
where
    T: Write + Read + Has + 'static,
{
    /// Creates the module around the host's register access handle.
    pub fn init(handle: T) -> Self {
        Self { handle }
    }
}

impl<T> UserData for Register<T>
where
    T: Write + Read + Has + 'static,
{
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Get", Self::get);
        methods.add_method("Set", Self::set);
        methods.add_method("Has", Self::has);
    }
}

impl<T> Register<T>
where
    T: Write + Read + Has + 'static,
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

    /// `Has(name)` — checks if register existts.
    fn has(_: &mlua::Lua, this: &Register<T>, name: String) -> Result<bool> {
        this.handle.has(name)
    }
}
