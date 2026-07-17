use ferrowl_lua_derive::Module;
use mlua::{Result, UserData};

/// Lua module `C_Time`: elapsed time since module creation.
///
/// Exposed Lua methods: `Get` (seconds) and `GetMs` (milliseconds).
#[derive(Module)]
#[module = "C_Time"]
pub struct Time {
    start: std::time::Instant,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl UserData for Time {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Get", Time::get);
        methods.add_method("GetMs", Time::get_ms);
    }
}

impl Time {
    fn get(_: &mlua::Lua, this: &Time, _: ()) -> Result<u64> {
        Ok(std::time::Instant::now()
            .duration_since(this.start)
            .as_secs())
    }

    fn get_ms(_: &mlua::Lua, this: &Time, _: ()) -> Result<u128> {
        Ok(std::time::Instant::now()
            .duration_since(this.start)
            .as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::Time;

    #[test]
    /// SC-R-017 — C_Time is measured from the moment its context is built, and building a fresh
    /// context (a fresh module) resets the origin to zero.
    fn ut_time_measured_from_construction_and_resets_on_rebuild() {
        let lua = mlua::Lua::new();
        let t1 = Time::default();
        // Freshly built: elapsed is ~0.
        assert!(Time::get_ms(&lua, &t1, ()).unwrap() < 40);

        std::thread::sleep(std::time::Duration::from_millis(60));
        let elapsed = Time::get_ms(&lua, &t1, ()).unwrap();
        assert!(
            elapsed >= 40,
            "elapsed should have advanced, got {elapsed}ms"
        );

        // Rebuilding the context builds a new Time whose origin is reset to now.
        let t2 = Time::default();
        assert!(
            Time::get_ms(&lua, &t2, ()).unwrap() < elapsed,
            "a rebuilt clock must start from zero, not inherit the old origin"
        );
    }
}
