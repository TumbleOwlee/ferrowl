use crate::{Error, Result, Script, module::LogLevel, module::LogSink, module::Module};
use mlua::{Lua, StdLib, UserData};
use std::{collections::HashMap, hash::Hash};

/// Lua context handling module and script loading
#[derive(Default)]
pub struct Context<K>
where
    K: Hash + Eq + Default,
{
    /// Native lua context
    lua: Lua,
    /// Collection of all loaded lua scripts
    scripts: HashMap<K, Script>,
}

#[allow(dead_code)]
impl<K> Context<K>
where
    K: Hash + Eq + Default,
{
    /// Add a new module to the lua context
    pub fn add_module<T>(&mut self, value: T) -> Result<()>
    where
        T: 'static + Module + UserData,
    {
        let globals = self.lua.globals();
        globals.set(T::module(), value)
    }

    /// Enable the standard libraries a sim script is allowed to use.
    ///
    /// Deliberately **not** `StdLib::ALL_SAFE`: that set only drops FFI and `debug`, leaving `io`,
    /// `os` (including `os.execute`), and `package`/`require` reachable -- so any script in a
    /// device or session config could read/write files and spawn processes, and loading a config
    /// would be equivalent to running an untrusted program. Sim scripts model device behavior;
    /// they have no legitimate need for the filesystem, the shell, or dynamic library loading.
    ///
    /// Only the pure computation libraries are kept (`string`, `table`, `math`, `utf8`,
    /// `coroutine`). Clock access, which a sim genuinely needs, is provided by the sandboxed
    /// `C_Time` module instead of `os`.
    ///
    /// `mlua` constructs a `Lua` with `ALL_SAFE` already loaded, so `load_std_libs` can only add
    /// libraries, never remove them -- the unwanted ones (`io`, `os`, `package`) and the base
    /// library's dynamic-code loaders (`load`, `loadfile`, `dofile`, `require`), none of which a
    /// `StdLib` flag can gate off, are therefore removed by clearing them from the globals.
    pub fn enable_stdlib(&mut self) -> Result<()> {
        let safe = StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE;
        self.lua.load_std_libs(safe)?;
        let globals = self.lua.globals();
        for name in [
            "io",
            "os",
            "package",
            "load",
            "loadfile",
            "dofile",
            "loadstring",
            "require",
        ] {
            globals.set(name, mlua::Value::Nil)?;
        }
        Ok(())
    }

    /// Override the global `print` so output goes to the host log instead of stdout
    /// (stdout would corrupt the TUI alternate screen). Mirrors real print semantics:
    /// arguments are converted with tostring semantics and joined by tabs.
    pub fn redirect_print<S: LogSink + 'static>(&mut self, sink: S) -> Result<()> {
        let f = self
            .lua
            .create_function(move |_, args: mlua::Variadic<mlua::Value>| {
                let line = args
                    .iter()
                    .map(|v| v.to_string()) // tostring semantics, honors __tostring
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .join("\t");
                sink.log(LogLevel::Info, &line);
                Ok(())
            })?;
        self.lua.globals().set("print", f)
    }

    /// Retrieve iterator over all loaded scripts
    pub fn iter<'a>(&'a self) -> std::collections::hash_map::Iter<'a, K, Script> {
        self.scripts.iter()
    }

    /// Retrieve mutable iterator over all loaded scripts
    pub fn iter_mut<'a>(&'a mut self) -> std::collections::hash_map::IterMut<'a, K, Script> {
        self.scripts.iter_mut()
    }

    /// Execute a loaded script specified by specific key
    pub fn call(&mut self, key: &K) -> Result<()> {
        match self.scripts.get_mut(key) {
            Some(script) => script.exec(),
            None => Ok(()),
        }
    }

    /// Execute a loaded script specified by specific key while skipping it if it has been executed
    /// in the last timeframe of given duration
    pub fn refresh(&mut self, key: &K, since: std::time::Duration) -> Result<()> {
        match self.scripts.get_mut(key) {
            Some(script) if script.since_last_execution() >= since => script.exec(),
            _ => Ok(()),
        }
    }

    /// Execute all loaded scripts
    pub fn call_all(&mut self) -> std::result::Result<(), Vec<Error>> {
        let errors: Vec<_> = self
            .iter_mut()
            .map(|(_, v)| v.exec())
            .filter(|r| r.is_err())
            .map(|e| e.err().unwrap())
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Execute all loaded scripts while skipping all scrips executed in the last timeframe of
    /// given duration
    pub fn refresh_all(
        &mut self,
        since: std::time::Duration,
    ) -> std::result::Result<(), Vec<Error>> {
        let errors: Vec<_> = self
            .iter_mut()
            .filter(|(_, v)| v.since_last_execution() >= since)
            .map(|(_, v)| v.exec())
            .filter(|r| r.is_err())
            .map(|e| e.err().unwrap())
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Load the script and store it under the given key unless another script is already loaded
    /// for the key
    pub fn load_script(&mut self, key: K, script: &str) -> Result<()> {
        let func = self.lua.load(script).into_function()?;
        if let std::collections::hash_map::Entry::Vacant(e) = self.scripts.entry(key) {
            e.insert(Script::init(func));
            Ok(())
        } else {
            Err(mlua::Error::BindError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Context;
    use std::time::Duration;

    fn key(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn ut_load_script_rejects_invalid_lua() {
        let mut ctx = Context::<String>::default();
        assert!(
            ctx.load_script(key("bad"), "this is not ! valid lua")
                .is_err()
        );
    }

    #[test]
    fn ut_load_script_rejects_duplicate_key() {
        let mut ctx = Context::<String>::default();
        assert!(ctx.load_script(key("a"), "local x = 1").is_ok());
        // Second load under the same key must not overwrite; it returns a bind error.
        assert!(ctx.load_script(key("a"), "local y = 2").is_err());
    }

    #[test]
    fn ut_call_missing_key_is_ok() {
        let mut ctx = Context::<String>::default();
        // No script registered for the key: nothing to run, so it succeeds vacuously.
        assert!(ctx.call(&key("nope")).is_ok());
    }

    #[test]
    fn ut_call_runs_script_and_surfaces_runtime_error() {
        let mut ctx = Context::<String>::default();
        ctx.load_script(key("ok"), "local x = 1 + 1").unwrap();
        ctx.load_script(key("boom"), "error('kaboom')").unwrap();
        assert!(ctx.call(&key("ok")).is_ok());
        let err = ctx.call(&key("boom")).unwrap_err();
        assert!(err.to_string().contains("kaboom"));
    }

    #[test]
    fn ut_sandbox_denies_filesystem_shell_and_dynamic_loading() {
        // A script in a config is untrusted input; the sandbox must not give it the filesystem,
        // the shell, or a way to pull in more code. Each of these globals must be absent.
        let mut ctx = Context::<String>::default();
        ctx.enable_stdlib().unwrap();
        // The whole table/global is gone, so `os.execute` et al. are unreachable -- an indexing
        // attempt would even throw "index a nil value" rather than return nil.
        for global in [
            "io",
            "os",
            "package",
            "require",
            "load",
            "loadfile",
            "dofile",
            "loadstring",
        ] {
            ctx.load_script(key(global), &format!("assert({global} == nil)"))
                .unwrap();
            assert!(
                ctx.call(&key(global)).is_ok(),
                "sandbox leaks `{global}` to scripts"
            );
        }
    }

    #[test]
    fn ut_sandbox_keeps_pure_computation_libraries() {
        let mut ctx = Context::<String>::default();
        ctx.enable_stdlib().unwrap();
        ctx.load_script(
            key("pure"),
            "assert(string.upper('a') == 'A'); assert(math.floor(1.5) == 1); \
             assert(table.concat({'x'}) == 'x')",
        )
        .unwrap();
        assert!(ctx.call(&key("pure")).is_ok());
    }

    #[test]
    fn ut_call_all_collects_errors() {
        let mut ctx = Context::<String>::default();
        ctx.load_script(key("ok"), "local x = 1").unwrap();
        ctx.load_script(key("boom"), "error('x')").unwrap();
        let errs = ctx.call_all().unwrap_err();
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn ut_refresh_skips_recently_executed_script() {
        let mut ctx = Context::<String>::default();
        // A failing script that would error if executed.
        ctx.load_script(key("boom"), "error('x')").unwrap();
        // Just loaded, so its last-execution age is ~0; a one-hour window skips it entirely,
        // proving the throttle: no execution means no error.
        assert!(ctx.refresh(&key("boom"), Duration::from_secs(3600)).is_ok());
        // Without the throttle the same script does run and surfaces its error.
        assert!(ctx.call(&key("boom")).is_err());
    }
}
