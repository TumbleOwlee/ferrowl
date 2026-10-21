pub mod traits;

use crate::module::StatusSource;
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
    T: Write + Read + Has + StatusSource + 'static,
{
    handle: T,
}

impl<T> Register<T>
where
    T: Write + Read + Has + StatusSource + 'static,
{
    /// Creates the module around the host's register access handle.
    pub fn init(handle: T) -> Self {
        Self { handle }
    }
}

impl<T> UserData for Register<T>
where
    T: Write + Read + Has + StatusSource + 'static,
{
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Get", Self::get);
        methods.add_method("Set", Self::set);
        methods.add_method("Has", Self::has);
        methods.add_method("ConnectionStatus", |_, this, ()| {
            this.handle.connection_status()
        });
    }
}

impl<T> Register<T>
where
    T: Write + Read + Has + StatusSource + 'static,
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

    /// `Has(name)` — checks if register exists.
    fn has(_: &mlua::Lua, this: &Register<T>, name: String) -> Result<bool> {
        this.handle.has(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBuilder;
    use crate::module::ConnectionStatus;
    use crate::module::test_support::MockHost;

    #[test]
    /// SC-R-082 — `C_Register:ConnectionStatus()` returns the host's status on every call.
    fn ut_register_connection_status_returns_host_status() {
        let host = MockHost::default();
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(Register::init(host.clone()))
            .with_script(
                "s".to_string(),
                r#"C_Register:Set("st", C_Register:ConnectionStatus())"#,
            )
            .build()
            .expect("build context");
        for (status, want) in [
            (ConnectionStatus::Connected, "CONNECTED"),
            (ConnectionStatus::Reconnecting, "RECONNECTING"),
            (ConnectionStatus::Disconnected, "DISCONNECTED"),
        ] {
            host.set_status(status);
            ctx.call_all().expect("run");
            assert!(
                matches!(host.get("st"), Some(ValueType::String(s)) if s == want),
                "want {want}"
            );
        }
    }
}
