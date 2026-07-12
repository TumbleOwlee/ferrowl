# Infra domain

Crates: `ferrowl-ring`, `ferrowl-util`. App-level: `ferrowl/src/instance/`, `ferrowl/src/config/`, `ferrowl/src/migrate.rs`, `ferrowl/src/main.rs` (session/tab bootstrap), build/CI config at repo root.

Snapshot: v0.4.13. Update this file when behavior it documents changes (see root `CONTRIBUTING.md`).

## 1. `ferrowl-ring` — `Ring<T, CAP>`

Single-file crate, zero dependencies.

- `Ring<T, const CAP: usize>` (`ferrowl-ring/src/lib.rs:14`): fields `buf: [Option<T>; CAP]`, `head: usize`, `len: usize`. Storage is a single inline array — no heap alloc for the container itself.
- API: `new()`, `capacity()` (const), `len()`, `is_empty()`, `is_full()`, `push(item)` (evicts oldest when full, no-op if `CAP==0`), `peek()` (oldest ref), `iter()`/`iter_mut()` (both `DoubleEndedIterator`, oldest-first, `.rev()` for newest-first, via `split_at(head)`), `peek_n(n) -> Vec<&T>`, `pop()` (removes+returns oldest), `pop_n(n)`, `clear()`.
- `Default`, `FromIterator<T>` (keeps only the last `CAP` items if more are collected).
- No `T: Copy`/`Clone` bound — fully generic, no unsafe code.
- Thread-safety: `Ring` has no internal locking; auto-traits derive from `T`. Always wrapped externally in the app: `type SharedLog = Arc<tokio::sync::RwLock<LogRing>>`.
- Formerly a log-specific crate (`ferrowl-log`); timestamping/line-truncation moved to callers.
- **N in practice**: `pub const LOG_SIZE: usize = 80` (`ferrowl/src/app/mod.rs:44`), used as `Ring<(u64, Level, String), LOG_SIZE>` inside `LogRing` (`ferrowl/src/app/mod.rs:85`). `headless.rs` redefines `const LOG_PEEK: usize = 80` locally (`ferrowl/src/headless.rs:42`, to avoid depending on `crate::app`) — **must be kept in sync manually** with `LOG_SIZE`. Verified: both are still 80.
- Tests: 12 `ut_*` unit tests (push/peek/pop, generic-over-String/tuple, full-capacity holds exactly CAP not CAP-1, overflow eviction order, peek_n/pop_n counts, clear+reuse, wraparound after pops, iter_mut forward+reverse, from_iter truncation, drop-on-evict via `Rc` strong-count assertions).

## 2. `ferrowl-util`

Files: `lib.rs`, `convert.rs`, `tokio.rs`.

**Macros/traits (`ferrowl-util/src/lib.rs`)**:
- `str!($a)` (`:21`) — `#[macro_export]`, expands to `$a.to_owned()`.
- `Expect<F>` trait (`:40`), generic impl for any `Result<T,E>`: `.panic(f: FnOnce(E)->String)` — like `.expect()` but the message is built from the actual error.
- `async_cloned!($($n),+; $body)` (`ferrowl-util/src/lib.rs:75`) — `#[macro_export]`, clones each named binding then wraps `$body` in `async move`.

**`convert.rs` — `Converter` (config file I/O helper, `ferrowl-util/src/convert.rs:47`)**:
- `FileType` (`ferrowl-util/src/convert.rs:12`): `Toml | Json`, `ValueEnum` (clap-integrated). `FileType::from_path(path)` infers from extension, case-insensitive.
- `Error`: `Serialize(String) | Deserialize(String)` (`thiserror`).
- `Converter::load<T: DeserializeOwned>(path, ty)` — read + deserialize.
- `Converter::save<T: Serialize>(value, path, ty)` — serialize. For TOML, routes through `serde_json::Value` first and normalizes via `json_to_toml` — needed because `arbitrary_precision` (pulled in transitively by `rust-ocpp`/`rust_decimal`) makes `serde_json::Number` serialize as a `{"$serde_json::private::Number": "…"}` wrapper that TOML would otherwise emit as a bogus sub-table.
- `Converter::convert<T>(src, src_type, dest, dest_type)` — round-trips a file between TOML/JSON via `T`.
- `json_to_toml(&Value) -> Result<toml::Value, Error>`: `null` errors at top level/in arrays (TOML has no null), drops `null` object fields silently; numbers: i64 preferred, u64 > i64::MAX falls back to `toml::Value::Float`; recurses arrays/objects.
- 18 `ut_*` tests, incl. round-trip, embedded `serde_json::Value` number normalization, malformed/missing file errors, u64-overflow→float, null handling.

**`ferrowl-util/src/tokio.rs` — "tracked" task spawning** (a lightweight join registry, not a panic-handling supervisor):
- Internal `Joinable` trait + impl for `JoinHandle<Output>`: `join()` awaits only `if !self.is_finished()`.
- `Context(HashMap<&'static str, Vec<Box<dyn Joinable>>>)` in a process-global `static CONTEXT: Lazy<Mutex<Context>>` (`ferrowl-util/src/tokio.rs:39`). `GLOBAL_CONTEXT: &str = ""` (`:34`) — default context name.
- `spawn_detach(future)` (`ferrowl-util/src/tokio.rs:62`) / `spawn_detach_with_context(ctx, future)` (`:90`): `tokio::spawn`s, stores the resulting `JoinHandle` (as `Box<dyn Joinable>`) in `CONTEXT` under the given name — **not returned to caller**. Caller must `.await` the spawn call itself (it's `async fn`; awaiting just registers, doesn't run the task inline).
- `join_all()` (`ferrowl-util/src/tokio.rs:131`) / `join_all_of_context(ctx)` (`:179`): drain the whole context (or one named context), `.join()` every handle, looping until none remain (guards against new tasks added concurrently while draining, per its own doc caveat — no guarantee no more tasks are added after returning).
- "Tracked" = registered in a named join registry for later bulk-await; **no panic isolation/catch_unwind involved**.
- **Notably unused in the app**: grep across `ferrowl/src` found zero usages of `spawn_detach`/`join_all` — the app crate uses plain `tokio::spawn` throughout. The mechanism exists in `ferrowl-util` but isn't wired into `ferrowl/src` app-level infra; treat as available-but-dormant, not load-bearing.
- Deps: async-trait, clap(derive), futures-util, once_cell, serde(derive), serde_json, thiserror 2.0, tokio(rt,sync,time), toml 0.9.11.

## 3. `Instance<T>` lifecycle (`ferrowl/src/instance/`)

**Modbus-only** — not shared with OCPP. OCPP has its own separate client/server lifecycle under `module/ocpp/{client,server}`; no common `Instance` abstraction spans both.

- `Instance<T: KeyParams>{ builder: Builder<T>, handle: Option<Handle> }` (`ferrowl/src/instance/mod.rs:20`).
- Constructors (`ferrowl/src/instance/mod.rs:34` onward): `with_tcp_client`, `with_rtu_client`, `with_tcp_server`, `with_rtu_server`, each wrapping the matching `ferrowl_modbus::{tcp,rtu}::{ClientBuilder,ServerBuilder}`.
- `active()` (`ferrowl/src/instance/mod.rs:26`): `true` iff `handle.is_some() && !handle.is_finished()`.
- `start<L,S>(log, status)` (`ferrowl/src/instance/mod.rs:78`): errors `InstanceError::AlreadyActive` if a handle exists and isn't finished; otherwise matches on `Builder` variant, spawns via the modbus builder's `.spawn(...)`, stores the resulting `Handle::Client{handle, sender}` (creates `mpsc::channel(10)` for commands) or `Handle::Server{handle}`.
- `stop()` (`ferrowl/src/instance/mod.rs:142`): errors `InstanceError::NotRunning` if `handle.is_none()`. Sends `ferrowl_modbus::Command::Terminate` via `send_command`; if that succeeds, sleeps **100ms** (hardcoded) as a grace period. Then takes the handle: if already finished, treats as `Ok(Ok(()))`; else `.abort()`s and awaits the `JoinHandle`. Result mapping: `Ok(Ok(_))→Ok(())`, `Ok(Err(e))→Err(e.into())` (propagates `ferrowl_modbus::Error`), `Err(e) if e.is_cancelled()→Ok(())` else `Err(InstanceError::CancelFailed)`.
- `send_command(command)` (`ferrowl/src/instance/mod.rs:194`): errors `NotRunning` if no handle; only `Handle::Client` accepts commands (via its `mpsc::Sender`), `Handle::Server` → `InstanceError::InvalidOperation`.
- **Restart semantics**: the same instance can be restarted after stopping — `start()` just checks the current handle isn't active, so calling `start()` again after `stop()` re-spawns cleanly (the builder is retained, not consumed).
- `InstanceError`: `AlreadyActive`, `NotRunning`, `CancelFailed`, `SendError(SendError<ferrowl_modbus::Command>)`, `InvalidOperation`.
- `Error`: `Net(#[from] ferrowl_modbus::Error)` | `Instance(#[from] InstanceError)`.
- `Handle`: `Server(ServerHandle{handle: JoinHandle<Result<(),ferrowl_modbus::Error>>})` | `Client(ClientHandle{handle, sender: Sender<Command>})`. `is_finished()` dispatches to the inner `JoinHandle::is_finished()`.
- `Builder<T: KeyParams>` enum wraps `ferrowl_modbus::{tcp,rtu}::{ClientBuilder,ServerBuilder}<T>`.
- `ClientConfig<T,Config>{config: Arc<RwLock<Config>>, operations: Arc<RwLock<Vec<Operation>>>, memory: Arc<parking_lot::RwLock<Memory<Key<T>>>>}`; `ServerConfig<T,Config>{config, memory}` (no operations).
- Tests: `start_twice_is_already_active`, `stop_never_started_is_not_running`, `send_command_never_started_is_not_running`, `send_command_on_server_is_invalid_operation` (constructs `Handle::Server` directly, no real I/O), `graceful_stop_deactivates_instance`, `active_reflects_finished_task` (drives task to natural completion via `Command::Terminate` + polling before calling `stop()`, exercising the "already finished" cleanup path).

## 4. `migrate` subcommand (`ferrowl/src/migrate.rs`)

Migrates pre-rewrite (≤0.3.9) `modbus-cli-rs` config → current `DeviceConfig`. Invoked via `ferrowl migrate --input FILE --output FILE`; dispatched before the tokio runtime is even created (`ferrowl/src/main.rs:371-374`).

**Old vs new field mapping**:

| Aspect | v0.3.9 | current |
|---|---|---|
| Holding read code | `read_code = 3` | `read_code = 4` |
| Input read code | `read_code = 4` | `read_code = 3` |
| Little-endian | `type = "U32le"` | `type="U32", endian="Little"` |
| Lua hook | `on_update` | `update` |
| Contiguous ranges | `[[contiguous_memory]]` | `[read_ranges]` |
| Connect delay | `delay_after_connect_ms` | `delay_ms` |

- `LegacyConfig`: `history_length`, `interval_ms`, `delay_after_connect_ms`, `timeout_ms`, `contiguous_memory: Vec<LegacyMemoryRange>`, `definitions: BTreeMap<String, LegacyRegisterDef>`.
- `LegacyAddress` is `#[serde(untagged)]` int-or-`"0xHEX"`-string, `resolve()` strips `0x`/`0X` and parses hex.
- `LegacyRegisterDef`: `slave_id`, `read_code` (default 3), `address`, `length` (default 1), `access` (default "ReadWrite"), `type` (value type), `reverse: bool`, `resolution` (default 1.0), `virtual` (is_virtual), `description`, `on_update`, `values: Vec<LegacyValue>` (untagged `Named{name,value}` or `Bare(i64)` — bare ints get warned and stringified as their own name), `default: Option<LegacyScalar>` (untagged int/float/text).
- `migrate_read_code(old: u8)`: `1→Coil, 2→DiscreteInput, 3→HoldingRegister, 4→InputRegister`, else `Err`.
- `parse_type(s)`: strips `"le"` suffix → `EndianCfg::Little`, else `Big`. `PackedAscii|LooseAscii|PackedUtf8|LooseUtf8 → ValueType::Ascii` (UTF-8 has no current equivalent, warned).
- `convert_ranges`: merges all `contiguous_memory` ranges of the same function code into one comma-separated string per code; non-zero `slave_id` in a range is dropped with a warning (no per-slave concept in `read_ranges`).
- `convert_def`: `reverse=true` and Utf8 types generate warnings but still convert (Ascii / dropped byte-swap); address > `u16::MAX` is a hard `Err` (register skipped, not aborted-run).
- `convert(legacy)` builds full `(DeviceConfig, Vec<String>)`: `version: Some(crate::config::VERSION)`, `reconnect: None` (legacy predates auto-reconnect, falls back to `DEFAULT_RECONNECT`), `log_file: None`, runs `device.migrate_update_scripts()` (folds `on_update` into `scripts: Vec<ScriptDef>`, one script per register named after the register).
- `run(args: &MigrateArgs)`: infers I/O `FileType` from extension, `std::process::exit(1)` on unrecognized extension or load/save failure; warnings printed to stderr with `warning:` prefix, success message on stderr with output path.
- Both JSON and TOML supported for both input and output, independently selectable per file extension.
- 8 tests incl. a full round-trip sample (`ut_convert_full_sample`) asserting read-code swap, endian mapping, range merging, `on_update`→scripts migration, and every warning category fires.

## 5. Top-level session loading (CLI → live tabs)

- `CliArgs` (`ferrowl/src/cli.rs:14`): `command: Option<SubCommand>` (`Migrate`|`Run`), `modules: Vec<String>` (`--module key=val,...`), `sessions: Vec<String>` (`--session FILE`, repeatable), `devices: Vec<String>` (`--device FILE`), `demo: bool`.
- `RunArgs` (headless subcommand, `ferrowl/src/cli.rs:62`) adds `ocpp: Vec<String>` (`--ocpp key=val,...`), `duration: Option<u64>`, `log_file: Option<String>`, `exit_on_error: bool`. `RunArgs::module_specs()`/`ocpp_specs()` delegate to an internally-constructed `CliArgs` (`as_cli_args()`) so TUI and headless paths share identical resolution logic.
- `CliArgs::module_specs()`: for each `--session` path, `config::load_session(path)`; for each entry in `session.modules` (`Vec<serde_json::Value>`), dispatches on `"type"` field (default `"modbus"` for backward compat with type-less old files) — `"modbus"`→deserialize as `ModuleSpec`, `"ocpp"`→skipped here (handled by `ocpp_specs`), other→hard `Err("unsupported module type")`. Then appends specs from `--module` flags (key=val parser) and one spec per `--device` flag (hardcoded client @ `127.0.0.1:5020`).
- `CliArgs::ocpp_specs()`: same session-file scan, filters `"type"=="ocpp"` entries into `OcppModuleSpec`.
- `build_tabs(args)` (`ferrowl/src/main.rs:238`): if `--demo`, builds 8 hardcoded demo tabs (2 modbus + 6 OCPP across v1.6/v2.0.1/v2.1 × client/server), starts each via `tab.view.handle_command("start")`. Otherwise resolves both modbus and OCPP specs, **dedupes names across both module types together** in creation order (auto-suffixes `" (2)"`, `" (3)"`, …) so a session with duplicate names still yields distinct tabs/`C_Module` registry entries; per-spec device config load failure → `eprintln!` warning + `continue` (module skipped, doesn't abort startup); for OCPP a missing/unreadable device file falls back to `OcppDeviceConfig::default()` rather than skipping.
- `session_sim_config(paths)` (`ferrowl/src/main.rs:221`): resolves session-level Lua scripts + sim interval across every `--session` file: scripts **concatenate** in file order, interval is **last file wins**; a file that fails here is silently skipped (already validated earlier by `build_tabs`).
- `main()` (`ferrowl/src/main.rs:368`): `Migrate` subcommand short-circuits before runtime creation; `Run` subcommand runs headless via `headless::run` and calls `std::process::exit(code)`; otherwise builds a multi-threaded tokio `Runtime`, installs a panic hook that releases the alternate screen before delegating to the default hook, then `build_tabs` → `App::new(tabs, session_scripts, session_interval)` → `app.run()`.

**Session schema** (`ferrowl/src/module/modbus/config/session.rs`, re-exported via `ferrowl/src/config/mod.rs`):

```
Session { version: Option<String>, modules: Vec<serde_json::Value>, scripts: Vec<ScriptDef>, interval: f64 }
```

- `version`: stamped on save from `crate::config::VERSION = env!("CARGO_PKG_VERSION")`; `#[serde(default, skip_serializing_if="Option::is_none")]` — purely informational today, "enables future compatibility shims", **no migration logic currently keys off it**.
- `modules`: opaque `Vec<Value>`, each expected to carry a `"type"` field for dispatch (default `"modbus"` if absent, for pre-multi-module-type files).
- `scripts`: `#[serde(default, skip_serializing_if="Vec::is_empty")]` — old files without it load as empty.
- `interval`: `#[serde(default = "default_interval")]` = 1.0. `Session::interval_duration()` delegates to shared `config::sanitize_interval_secs(value, default=1.0, min=0.0)` (no floor for session interval).
- `ModuleSpec{name, device: String, role: Role (default Server), endpoint: Endpoint}`; `Role{Client, Server(default)}`; `Endpoint` tagged `transport`: `Tcp{ip,port} | Rtu{path, baud_rate(default 19200), parity, data_bits, stop_bits}` (all RTU extras optional).
- `config::VERSION` (`ferrowl/src/config/mod.rs:24`) = `env!("CARGO_PKG_VERSION")`, stamped into both `DeviceConfig::version` and `Session::version` on save.
- `config::sanitize_interval_secs(value, default_secs, min_secs)` (`ferrowl/src/config/mod.rs:33`): shared NaN/negative/zero guard + floor, used by `Session::interval_duration`, `DeviceConfig::script_interval_duration`, `OcppDeviceConfig::script_interval_duration`.
- `config::load_device` additionally calls `device.migrate_update_scripts()` after load — runs on **every** load, not just via the `migrate` subcommand, so hand-edited/older device files self-heal on load. `load_ocpp_device`/`load_session` are plain loads, no post-processing.
- `ConfigError{UnknownFormat(String), Io(String)}`; `file_type()` wraps `FileType::from_path`.

## 6. Build / CI / versioning

- **Workspace** (`Cargo.toml`): `resolver="3"`, 12 members: `ferrowl-lua, ferrowl-store, ferrowl-syntax, ferrowl-codec, ferrowl-ring, ferrowl-modbus, ferrowl-ocpp, ferrowl-util, ferrowl-ui, ferrowl, ferrowl-ui-derive, ferrowl-lua-derive`. `[workspace.dependencies] parking_lot = "0.12"`.
- **Profiles**: `[profile.release] incremental = true` (speeds up repeated release builds during dev); `[profile.fastrel] inherits = "release", opt-level = 1, debug = false` — usage `cargo build --profile fastrel`, a fast-iteration profile.
- **Toolchain**: `rust-toolchain.toml` → `[toolchain] channel = "stable"` (no pinned patch version, tracks current stable). `ferrowl/Cargo.toml` `edition = "2024"`.
- **Versioning**: all 12 crates share version `0.4.13` — uniform lockstep versioning across the whole workspace.
- **Woodpecker CI**:
  - `.woodpecker/check.yml`: single `check` step, image `rust:latest`, runs `cargo check` then `cargo test`; triggers on `push` and `manual`. The PR/push gate.
  - `.woodpecker/nightly.yml`: triggers only on tag `refs/tags/nightly`. `build`: installs `mingw-w64`/`zip`, adds `x86_64-pc-windows-gnu` rustup target, runs `cargo check`, `cargo test`, `cargo build --release`, `cargo build --release --target x86_64-pc-windows-gnu` (Linux + Windows-cross binaries). `delete-nightly`: GitHub API (curl+jq) generates release notes, deletes any prior `nightly` release before re-publishing. `upload`: `woodpeckerci/plugin-release` publishes as a prerelease titled "Nightly" with sha256+md5 checksums.
- **lefthook** (`.lefthook.yml`): `pre-commit`, `piped: true` (sequential, stop on first failure), two commands globbed to `*.rs`: `fmt` → `cargo fmt -- --check`, `clippy` → `cargo clippy -- -D warnings` (warnings-as-errors). No `pre-push` hooks defined.
- **Testing conventions** (per `CONTRIBUTING.md`): pre-submit checklist `cargo check`, `cargo test --workspace`, `cargo clippy --workspace`, `cargo fmt --check`. Unit tests live in `#[cfg(test)]` modules next to the code.
- **The `ut_*` naming rule is NOT universal — don't assume it when grepping.** Of 1009 in-`src` test fns, 245 (24%) lack the prefix. It holds 100% in `ferrowl-codec`, `ferrowl-store`, `ferrowl-modbus`, `ferrowl-ocpp`, `ferrowl-lua`, `ferrowl-syntax`, `ferrowl-util`, `ferrowl-ring`. It does **not** hold in `ferrowl-ui` (116/118 unprefixed), `ferrowl-ui-derive` (21/21), `ferrowl-lua-derive` (3/3), or `ferrowl` (105/534). Follow `ut_*` for new tests; expect plain names in the UI/derive crates.
- **Integration tests do exist in `tests/` dirs** (13 files), despite the "tests live next to the code" phrasing: `ferrowl/tests/headless.rs`, `ferrowl-ocpp/tests/ws_loopback_{v16,v201,v21,security}.rs`, `ferrowl-modbus/tests/tcp_loopback.rs`, `ferrowl-ui/tests/render.rs`, `ferrowl-ui-derive/tests/{focus,overlay,table_entry}.rs`, `ferrowl-lua/tests/{modules,print}.rs`, `ferrowl-lua-derive/tests/module.rs`. `ferrowl/src/session_e2e_tests.rs` is an in-`src` e2e module using an `it_*` prefix. Cross-crate/loopback/render coverage belongs there, not inline.
- Dev workflow: `cargo build --release`, then `cargo run --release -- --demo`.
