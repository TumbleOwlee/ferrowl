use crate::ScriptState;

pub struct Script {
    state: ScriptState,
    func: mlua::Function,
}

impl Script {
    /// Create lua script state from native function handle
    pub fn init(func: mlua::Function) -> Self {
        Self {
            state: ScriptState::ok(),
            func,
        }
    }

    pub fn since_last_execution(&self) -> std::time::Duration {
        let now = std::time::Instant::now();
        now.duration_since(self.state.time_since())
    }

    pub fn exec(&mut self) -> crate::Result<()> {
        self.exec_with(())
    }

    /// Runs the script with `args` as the chunk's `...`.
    pub fn exec_with<A: mlua::IntoLuaMulti>(&mut self, args: A) -> crate::Result<()> {
        match self.func.call::<()>(args) {
            Ok(_) => {
                self.state = ScriptState::ok();
                Ok(())
            }
            Err(e) => {
                self.state = ScriptState::err(e.clone());
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Script;
    use mlua::Lua;

    #[test]
    /// SC-R-003 — a compiled script is invoked with no args when none are passed; a successful call is Ok, a failing one Err.
    fn ut_script() {
        let lua = Lua::new();

        let func = lua.load("local test = 1").into_function().unwrap();
        let mut script = Script::init(func);
        let result = script.exec();
        assert!(result.is_ok());

        let func = lua.load("func()").into_function().unwrap();
        let mut script = Script::init(func);
        let result = script.exec();
        assert!(result.is_err());
    }

    #[test]
    /// SC-R-003 — a script reads the arguments it is invoked with as its chunk's `...`.
    fn ut_exec_with_passes_args_as_varargs() {
        let lua = Lua::new();
        let func = lua
            .load("local a, b = ...; assert(a == 'x' and b == 7, 'bad args')")
            .into_function()
            .unwrap();
        let mut script = Script::init(func);
        assert!(script.exec_with(("x", 7)).is_ok());
        assert!(script.exec_with(("y", 7)).is_err());
        assert!(script.exec().is_err());
    }
}
