# Lua domain

Crates: `ferrowl-lua`, `ferrowl-lua-derive`. App-level wiring: sim threads under `ferrowl/src/lua.rs`, `ferrowl/src/module/ocpp/client/lua_sim.rs`, `ferrowl/src/module/ocpp/server/lua.rs`, `ferrowl/src/session_sim.rs`; script editor UI in `ferrowl/src/dialog/scripts.rs`.

Snapshot: v0.4.13. Update this file when behavior it documents changes (see root `CONTRIBUTING.md`).

## 1. `#[derive(Module)]` proc macro (`ferrowl-lua-derive`)

Attribute: `#[module = "C_Foo"]` (string literal, name-value), applied to a struct/enum (possibly generic, with `where` clauses). Generates:

```rust
impl<...> ferrowl_lua::module::Module for Ident<...> where ... {
    fn module() -> &'static str { "C_Foo" }
}
```

Works through generics/bounded generics. Compile errors: duplicate `#[module = …]` attributes; non-string-literal value; missing attribute entirely.

`Module` trait (`ferrowl-lua/src/module/mod.rs:24-27`): single method `fn module() -> &'static str` — the Lua global name a value is registered under via `Context::add_module`/`ContextBuilder::with_module` (sets `globals[T::module()] = value`).

## 2. Exhaustive Lua API surface

All modules are `mlua::UserData` global tables registered under their `#[module]` name. Every method is `add_method` (immutable `&self` receiver, `this:Method(...)` syntax).

| Module (Lua global) | Rust type | Method | Signature | Purpose |
|---|---|---|---|---|
| `C_Register` | `Register<T: Write+Read+Has>` | `Get(name)` | `(string) -> number\|string\|bool` | Read a register, raises on unknown/decode failure |
| | | `Set(name, value)` | `(string, value) -> ()` | Write a register |
| | | `Has(name)` | `(string) -> bool` | Check register existence |
| `C_Time` | `Time` | `Get()` | `() -> u64` | Seconds since module/context creation |
| | | `GetMs()` | `() -> u128` | Milliseconds since creation |
| `C_Log` | `Log<S: LogSink>` | `Info(line)` | `(string) -> ()` | Log at Info to host sink |
| | | `Warn(line)` | `(string) -> ()` | Log at Warning |
| | | `Error(line)` | `(string) -> ()` | Log at Error |
| `C_Test` | `Test` | `Assert(cond, msg)` | `(any, string) -> ()` | Raises `"assertion failed: {msg}"` if `cond` is Lua-falsy (nil/false) |
| | | `Fail(msg)` | `(string) -> ()` | Always raises `"assertion failed: {msg}"` |
| `C_Statics` | `Statics` | `Get(name)` | `(string) -> number\|string\|bool` | Read-only host-provided constant; errors on unknown key |
| `C_OCPP` (flat, `Ocpp<H>`) | `Ocpp<H: OcppHandle>` | `Get(name)` | `(string) -> value` | Read OCPP state field |
| | | `Set(name, value)` | `(string, value) -> ()` | Write OCPP state field |
| | | `<Action>(overrides?)` | `(table?) -> bool` | One method per host-supplied `OcppActions::actions()` name (version-specific set); dispatches/enqueues the action, `overrides` is a flat name→scalar table |
| `C_OCPP` client (`OcppClient<H>`) | adds: | `Connector(id)` | `(i64) -> Accessor` | Per-connector accessor with its own `Get`/`Set`/`<Action>` |
| | | `GetConnectors()` | `() -> [i64]` | List connector ids |
| `C_OCPP` server (`OcppServer<H>`) | | `GetChargingStations()` | `() -> [string]` | List connected station identities |
| | | `GetConnectors(cs)` | `(string) -> [i64]` | List connector ids for a station |
| | | `ChargingStation(cs)` | `(string) -> Accessor\|nil` | CS-level accessor for a station |
| | | `Connector(cs, id)` | `(string, i64) -> Accessor\|nil` | Per-connector accessor for a station |
| `Accessor<H>` (return of `Connector`/`ChargingStation`, not a global) | | `Get`/`Set`/`<Action>` | same shape as flat `C_OCPP` | Scoped state/action surface |
| `C_Module` (session-level only) | `ModuleDir` | `List()` | `() -> [string]` sorted | Names of every module currently in the session |
| | | `Get(name)` | `(string) -> ModuleHandle` | Resolve a module by name; raises `"unknown module '{name}'"` if absent |
| `ModuleHandle` (return of `C_Module:Get`) | | `Type()` | `() -> string` | Module kind, `"modbus"`/`"ocpp"` |
| | | `Role()` | `() -> string` | `"client"`/`"server"` |
| | | `Register()` | `() -> RegisterModule` | `C_Register`-shaped accessor; errors `"is not a modbus module"` otherwise |
| | | `OCPP()` | `() -> OcppClient/OcppServer` | `C_OCPP`-shaped accessor; errors `"is not an ocpp module"` otherwise |
| `print` (global, redirected) | n/a | `print(...)` | variadic → tostring/tab-joined | Redirected to host log sink instead of real stdout, avoids corrupting the TUI |

Notes:
- `ModuleHost` trait (type-erased per-module bridge): `kind()`, `role()`, `register_accessor(&Lua)`, `ocpp_accessor(&Lua)`. `ModuleDirectory` trait: `list()`, `resolve(name)`.
- `ModuleHandle` **re-resolves through the directory on every call** (not cached) — a module removed after `Get` makes subsequent calls raise "unknown module".
- `ValueType` is the cross-boundary dynamic type: `Int(i128) | Float(f64) | String(String) | Bool(bool) | Nil`, mapped to/from Lua Integer/Number/String/Boolean/Nil; any other Lua type (table, function, userdata) fails conversion.
- `OcppActions` trait: `actions() -> Vec<&'static str>` (static, per host type) and `dispatch(&self, action, args) -> bool`.
- `OcppClientHost`/`OcppServerHost` traits define `connector(id)`/`connectors()` and `stations()`/`connectors(cs)`/`station(cs)`/`connector(cs,id)` respectively — select `OcppClient` vs `OcppServer` shape at the Rust-generic level, not runtime dispatch.

## 3. Script execution model

- **VM**: `mlua` 0.11.4, features `["anyhow","lua54","vendored"]` — real Lua 5.4, vendored/compiled in, **sync (blocking) API only** — no `mlua` async feature.
- **Sandboxing**: `Context::enable_stdlib` calls `lua.load_std_libs(StdLib::ALL_SAFE)` — mlua's "safe" stdlib subset (no `io`, `os.execute`, `debug`, etc.). No further sandboxing — **no instruction/memory limit configured anywhere**.
- **Context/Script model**: `Context<K>` owns one `mlua::Lua` plus a `HashMap<K, Script>`. `Script` wraps an `mlua::Function` plus `ScriptState` (ok/err + last-execution `Instant`). `Script::exec` calls the compiled function with `()` args, no return value expected; on error, state flips to `Err`, `mlua::Error` propagates.
- **Execution APIs**: `Context::call(key)` runs one script; `call_all()` runs every script, collecting all errors into `Vec<Error>` (does not stop early); `refresh(key, since)`/`refresh_all(since)` skip scripts executed more recently than `since` (throttled tick).
- **Thread/async model**: `mlua::Lua` is `!Send`, so every sim owner spawns a **dedicated OS thread** (`std::thread::spawn`) that builds the `Context` inside the thread and loops until a stop `AtomicBool` flag is set:
  - Modbus: `run_sim` in `ferrowl/src/lua.rs:299-360`.
  - OCPP client: `run_client_sim` in `ferrowl/src/module/ocpp/client/lua_sim.rs:218-269`.
  - OCPP server: `run_server_sim` in `ferrowl/src/module/ocpp/server/lua.rs:224-269` — one thread for the whole server module, spanning all connected stations.
  - Session-level: `SessionSim::ensure` in `ferrowl/src/session_sim.rs:62-115`.
  - Each loop: sleeps in small chunks (`sleep_responsive`; 50ms for modbus/session, 25ms for OCPP) up to the configured interval so the stop flag is observed promptly, then calls `context.call_all()` (modbus, session) or `context.refresh_all(interval)` (OCPP client/server — throttled per-script).
  - `SimHandle`/`OcppSimHandle` wrap the stop flag + `JoinHandle`; `Drop` stops+joins the thread.
- **Error handling**: a per-script runtime error (`error(...)`, `C_Test:Assert`/`Fail` raising, malformed OCPP override tables) does **not** crash the sim thread or stop other scripts — errors are collected and continue; each is formatted and pushed to the module's log ring with a `[sim]`/`[lua]` prefix at `Error` level. If the `ContextBuilder` itself fails (duplicate script name, bad Lua syntax at load time), the whole sim thread logs `"[sim] failed to build Lua context: {e}"` / `"[lua] failed to build context: {e}"` and returns without looping.
- **Headless `--exit-on-error`**: `ferrowl run` exits code `2` if a drained log line starts with `SIM_ERROR_PREFIX = "[sim]"` — detection is purely string-prefix matching against what `emit`/session sim writes.
- **Register/OCPP access from Lua**: bridged via host-implemented traits, never direct memory access.
  - Modbus: `RegisterBridge` implements `Read`/`Write`/`Has` over `ModuleMemory` (a `parking_lot::RwLock<Memory<...>>`, locked synchronously) for fixed-address registers, and `VirtualStore` (`tokio::sync::RwLock<HashMap<String, Value>>`, accessed via `.blocking_read()/.blocking_write()` since the sim thread isn't a tokio worker) for virtual registers. Encoding goes through `ferrowl_codec::Register::decode/encode/encode_value`.
  - OCPP client: `ClientCsHandle<S>`/`ClientConnHandle<S>` read/write via a `ClientFields` trait over `Arc<RwLock<S>>` state, enqueue actions onto a `ScopedActionQueue` (`Arc<Mutex<VecDeque<(Scope,String,serde_json::Value)>>>`) drained by the owning view each refresh tick.
  - OCPP server: `CsHandle<V>`/`ConnHandle<V>` similarly, enqueueing onto `ServerActionQueue` keyed additionally by station identity.
  - Session-level `C_Module`: resolves per-tab hosts (`ModbusHost`, `OcppClientEntry<S>`, `OcppServerEntry<V>`) through `ModuleRegistry`, which `App::rebuild_registry` repopulates whenever tabs change.
  - `C_Log`/`print` route through a `LogSink` trait, implemented per host as `LuaLogSink` wrapping the module's ring log (+ optional file sink for modbus).

## 4. Script authoring

- **Definition**: `ScriptDef{name: String, code: String (default ""), enabled: bool (default true)}` — shared by modbus and OCPP device configs and the session config.
- **Storage**: inline in the device/session TOML/JSON config files as `scripts: Vec<ScriptDef>` — **not** external `.lua` files.
  - Modbus: `DeviceConfig.scripts` + `.script_interval: f64` (default 1.0s).
  - OCPP: `OcppDeviceConfig.scripts` + `.script_interval` (client role only, actually simulated).
  - Session-level: `Session.scripts`/`.interval`, aggregated across all `--session` files — last interval wins, scripts concatenated.
  - Legacy migration: `DeviceConfig::migrate_update_scripts` converts old per-register `update` Lua snippets into named, enabled entries in `scripts` on load.
- **UI editor**: `ScriptDialog` (`ferrowl/src/dialog/scripts.rs`) — shared TUI dialog (`:script` opens the view-specific dialog; `:script copy <tab-index>` copies another tab's script list). Layout: interval input → script table (name + On/Off, `t` toggles, `d` deletes w/ confirm, `c` toggles compact rows) → "New Script" name input → Lua-syntax `CodeInputField` (vim-modal editor, see `ui.md`) → read-only tail of the module's script log. `?` in Code/Normal mode opens `LuaHelpOverlay` — a scrollable per-context cheat sheet of the modules/methods above (`ScriptContext`s: `Modbus`, `OcppClient`, `OcppServer`, `Session`; shared sections always: `C_Time`, `C_Test`, `C_Log`, `print`).
- **Built-in globals besides `C_*`**: only the redirected `print`. No other host-injected globals; Lua's own `ALL_SAFE` stdlib subset is available for `local`, control flow, `string`, `table`, `math`, etc.
- **Examples**: no `.lua` example files ship in the repo — examples exist only as inline strings in Rust tests, e.g. `C_Register:Set("power", C_Register:Get("setpoint"))`, `C_OCPP:StartTransaction({ idTag = "ABC" })`, `C_Module:Get("m"):Register():Set("value", 42); C_Log:Info("session-script-ran")`.

## 5. Numeric limits / constants

| Constant | Value |
|---|---|
| `MIN_SCRIPT_INTERVAL_SECS` (modbus) | 0.05 s |
| `MIN_SCRIPT_INTERVAL_SECS` (OCPP) | 0.05 s |
| `default_script_interval()` (modbus & OCPP) | 1.0 s |
| Session sim default interval | 1.0 s |
| Sanitization rule (`config::sanitize_interval_secs`) | value used if finite & `>0`, floored to `min_secs`; else falls back to `default_secs` |
| Modbus sim thread poll granularity | 50 ms chunks |
| OCPP sim thread poll granularity | 25 ms chunks |
| Headless tick / log-drain period | 100 ms |
| Headless log peek depth | 80 lines |
| Headless exit-on-error error prefix | `"[sim]"` |
| OCPP default reply timeout (when `timeout_ms` unset) | 30 000 ms (default lives in `ferrowl-ocpp`, see `ocpp.md`) |
| Max scripts per module | **none enforced** — only duplicate-name rejection (`Context::load_script` → `mlua::Error::BindError`, and separately in the UI's `create_script`) |
| Script execution timeout / instruction-count / memory limit | **none configured** — `mlua::Lua` created via `Lua::default()`/`Lua::new()`, no `set_hook`/interrupt/memory-limit calls anywhere in the crate |
