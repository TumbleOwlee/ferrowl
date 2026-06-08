use crate::{Error, Result, Script, module::Module};
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

    /// Enable support of standard libraries in lua context
    pub fn enable_stdlib(&mut self) -> Result<()> {
        self.lua.load_std_libs(StdLib::STRING)?;
        self.lua.load_std_libs(StdLib::MATH)?;
        self.lua.load_std_libs(StdLib::TABLE)?;
        self.lua.load_std_libs(StdLib::ALL_SAFE)?;
        Ok(())
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
        self.iter_mut()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.exec())
            .find(|r| r.is_err())
            .unwrap_or(Ok(()))
    }

    /// Execute a loaded script specified by specific key while skipping it if it has been executed
    /// in the last timeframe of given duration
    pub fn refresh(&mut self, key: &K, since: std::time::Duration) -> Result<()> {
        self.iter_mut()
            .filter(|(k, v)| *k == key && v.since_last_execution() >= since)
            .map(|(_, v)| v.exec())
            .find(|r| r.is_err())
            .unwrap_or(Ok(()))
    }

    /// Execute all loaded scripts
    pub fn call_all(&mut self, since: std::time::Duration) -> std::result::Result<(), Vec<Error>> {
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
    fn ut_call_all_collects_errors() {
        let mut ctx = Context::<String>::default();
        ctx.load_script(key("ok"), "local x = 1").unwrap();
        ctx.load_script(key("boom"), "error('x')").unwrap();
        // Duration::ZERO means every script is eligible to run.
        let errs = ctx.call_all(Duration::ZERO).unwrap_err();
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
