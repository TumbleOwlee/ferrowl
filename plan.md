# Ferrowl: rename the workspace + build out the full multi-module TUI

## Context
The project is being rebranded to **Ferrowl**, and the application itself is being built out
from a disconnected demo into a real tool. Today the app crate (`cli`) is a demo: `main.rs`
spawns two `Module`s in a background runtime, then *separately* builds hardcoded register
`Definition`s against a throwaway `Memory`, with a sync loop that toggles between three
full-screen views via `ALT+k`. The two edit dialogs and the `TableView` are wired to nothing.

The goal is the real application: a tool that manages **multiple modules** (each a modbus
**client** reading an external server, or a **server** simulating a device), presented as
**tabs**. The active module shows a tab bar (top), a live register **table** (fill), a
scrollable **log** pane (bottom), and a **command inputline** (very bottom, vim-style `:`).
Selecting a register opens an **edit dialog** to change any part of it (and optionally write a
value); selection-type registers can **Add** values. Modules are created from **device-type
config files** + a per-instance **setup dialog** (or CLI/session file for cluster startup).

## Visual overview

### TUI layout
```
┌─ Ferrowl ─────────────────────────────────────────────────────────────────┐
│ ‹ evse-1 › │ evse-2 │ monitor-A                          tab bar · gt/gT  │
├───────────────────────────────────────────────────────────────────────────┤
│ Name      Addr Access Kind     Format Value Raw                           │
│ setpoint  0    RW     Holding  U16    32    [0x20]      register table    │
│ power     1    RW     Holding  U16    32    [0x20]      (fill · j/k)      │
│ current   2    RO     Input    I16    12    [0x0C]                        │
├───────────────────────────────────────────────────────────────────────────┤
│ 12:00:01 client connected                              log pane           │
│ 12:00:02 read holding [0,3) ok                         (scroll · Tab)     │
├───────────────────────────────────────────────────────────────────────────┤
│ :set power 100                                          command line · ‹:›│
└───────────────────────────────────────────────────────────────────────────┘
        Enter on a row →  edit dialog  (centered overlay)
        :e / :l         →  setup dialog (centered overlay)
```

### Crate graph (after rename)
```
                               ferrowl  (binary · the TUI)
   ┌──────────┬───────────┬───────────┬───────────┬───────────┬──────────────┐
   ▼          ▼           ▼           ▼           ▼           ▼              ▼
ferrowl-ui ferrowl-net ferrowl-reg ferrowl-lua ferrowl-log ferrowl-util ferrowl-derive
                │                                                        (proc-macro;
                ▼                                                         emits ferrowl_ui::*
            ferrowl-mem                                                   → used together w/ ui)

  real edges: ferrowl → every lib ;  ferrowl-net → ferrowl-mem
  ui↔derive are dev-only (examples).   NEW: add ferrowl-lua to ferrowl's deps.
```

### Runtime & data flow
```
   terminal                           ┌──────────────── tokio runtime ───────────────────┐
   keys │ ▲ draw                      │                                                  │
        ▼ │                           │   App async loop  (src/app.rs)                   │
  crossterm EventStream ──events────▶ │   ┌─────────────────────────────────────────┐    │
                                      │   │ snap = collect_snapshot().await ◄───────┼────┼─ .read().await
         render(snap) ◄───────────────┼───│ terminal.draw(|f| render(snap))  (sync) │    │   (Memory/Log)
                                      │   │ select!{ ev = events.next(); _ = tick } │    │
   dispatch(ev):                      │   └───────────────────┬─────────────────────┘    │
    gt/gT tab · Tab pane · ':' cmd    │      owns Vec<Module> │ :start/:stop/:set/edit   │
    Enter → dialog · Esc              │                       ▼                          │
                                      │              one Module per tab  (see below)     │
                                      └──────────────────────────────────────────────────┘
```

### Module internals + Lua simulation
```
 ┌──────────────────────────────── Module (per tab) ─────────────────────────────────┐
 │  register table:  name → ferrowl_reg::Register (addr · format · access · update)  │
 │                                                                                   │
 │        ┌───────── Memory  Arc<RwLock> ──────────┐                                 │
 │   async│  .read()/.write().await                │ sync  blocking_read/write       │
 │   ┌────┴───────┐   modbus I/O      ┌────────────┴───────┐   drives                │
 │   │  Instance  │◄── over network ──│   RegisterBridge    │◄────┐                  │
 │   │ client OR  │                   │   = C_Register      │     │ refresh_all()    │
 │   │  server    │                   │  Get*/Set(name,..)  │     │ every cycle      │
 │   └────────────┘                   └─────────────────────┘     │                  │
 │                                       exposed to Lua ─▶┌───────┴───────────────┐  │
 │   Log ring Arc<RwLock> ─▶ screen pane + per-module file│ sim thread + Lua      │  │
 │   (script errors land here)                            │ Context (not Send)    │  │
 │                                                        └───────────────────────┘  │
 └───────────────────────────────────────────────────────────────────────────────────┘
   update e.g.:  C_Register:Set("power", C_Register:GetInt("setpoint"))
```

### Config & startup flow
```
  --module key=val (repeatable) ─┐
  --session cluster.toml ────────┤
                                 ├─▶ resolve instances ─▶ build Module ─▶ auto-start (CLI)
  device-type files ─────────────┤                            ▲
   (registers + update Lua)      │                            │
  general config ────────────────┘          :l/:load <file> ──┘─▶ setup dialog ─▶ new tab
   ./config.toml ▸ ~/.config/ferrowl/config.toml ▸ --config       (name·transport·addr·role)
```

## Locked decisions
- **Rename to Ferrowl**: every crate becomes `ferrowl-<suffix>` (directories renamed via
  `git mv`, package names, and `ferrowl_*` imports). The app crate `cli` becomes package
  **`ferrowl`** with binary **`ferrowl`**. `ferrowl-derive` → `ferrowl-derive`. Done first as a
  prerequisite; all crate references below use the new names.
- **Async UI loop**: the event/render loop moves into the tokio runtime; `Memory`/`Log` read via
  `.read().await`. Snapshot before each `draw` (rendering stays sync).
- **Two config layers**: a *general app config* (globals: `history_length`, default timing, log
  file base) + *device-type config files* (one file = one device's register definitions +
  timing; **no** ip/port/role/name).
- **Role per instance** (client/server chosen in setup dialog + CLI), not in the device file.
- **Transports**: TCP **and** RTU (setup dialog/CLI adapt: TCP ip+port; RTU serial path+baud/…).
- **Module creation**: `:l`/`:load` a device-type file → setup dialog (name + transport + addr +
  role) → new tab. Multiple instances of one device type allowed.
- **CLI cluster startup**: repeatable `--module key=val` **and** `--session <file>`; CLI modules
  auto-start. General config discovery: `./config.toml` → `~/.config/ferrowl/config.toml` →
  `--config <path>`.
- **Commands**: `:q`/`:q!`, `:w`/`:write [path]`, `:l`/`:load [path]`, `:e`/`:edit` (open the
  module-tab setup/config dialog), `:n`/`:new` (open the new-module dialog), `:start`/`:stop`/
  `:restart`, `:set <reg> <value>`, `:log <file>`.
- **Fixed value sets use a `Selection`** (never a free-text input): transport, role, parity,
  data bits, stop bits, etc. Free-range fields (name, ip, port, baud, path, value) stay inputs.
- **A dialog with a validation error cannot be confirmed** — `Enter` only applies when the
  dialog fully validates; `Esc` always cancels.
- **Edit dialog applies both**: register metadata **and**, if a value is entered, a write —
  server → `Memory` write; client → `Instance::send_command`. Edit dialog also **creates** new
  registers (opened empty). Selection dialog **Add** appends a value.
- **Log**: display-only but **scrollable**; ring length from general config; every module also
  logs to **its own file** (base from `--log`/`:log <file>`, tab name as suffix).
- **Per-module Lua simulation**: each module owns a `ferrowl_lua::Context` (its own Lua state).
  Each register may define an optional `update` property (Lua code) run **every cycle**, enabling
  full automatic server simulation (e.g. EVSE: an `update` on `power` copies `setpoint` into it).
  Scripts access registers through the provided `C_Register` UserData
  (`GetInt/GetFloat/GetString/GetBool(name)`, `Set(name, value)`); `C_Time`/`C_Statics` also
  exposed. The `cli` (→ `ferrowl`) crate must add a dependency on `ferrowl-lua` (it has none today).
- **Reuse UI as-is**: `ferrowl_ui` already provides every element needed — **table, input,
  selection, text, button**. Build **no new widgets**; all views (tabs/log/command) and dialogs
  compose these via the `Widget<S,W>` wrapper + the `Focus` derive. The register table keeps the
  full **11-column** layout already implemented in `view::main` (the `TableHeader`).

## Dependency policy
Prefer this workspace's crates and a small set of established, well-maintained ecosystem crates
over custom code; do **not** add unmaintained crates or ones that don't pay for themselves in real
simplification. Concretely:
- **CLI**: `clap` (derive) — already a dep. The `--module key=val` field parser is a trivial
  in-house split (no extra crate).
- **Config (de)serialize**: `serde` + the workspace `ferrowl_util::Converter` (wraps `toml` +
  `serde_json`). No new serialization crate.
- **TUI / input**: `ratatui` + `crossterm` (already deps); enable crossterm's `event-stream`
  feature for the async loop.
- **Async**: `tokio` (already). To await the crossterm `EventStream`, add **one** stream adapter
  — `tokio-stream` (preferred, tokio-native) or `futures-util` — for `StreamExt::next`.
- **Lua**: via the workspace `ferrowl_lua` (wraps `mlua`); no direct `mlua` dep in the app.
- **Builders/getters**: `derive_builder` + `getset` (already used by the widgets/dialogs).
- **Config-path discovery** (`~/.config/ferrowl/…`): resolve with std `env`
  (`XDG_CONFIG_HOME`→`HOME`); adopt a tiny crate (`etcetera`/`dirs`) only if cross-platform
  correctness later demands it.
- **Per-module file logging**: std `std::fs` + `BufWriter` (no logging framework).
- **Errors**: `anyhow` for app-level (already); keep the existing hand-rolled typed errors, add
  `thiserror` only if they grow.
- **Custom derive macros are encouraged** in `ferrowl-derive` when they remove substantial
  boilerplate (as `Focus` does) — e.g. a derive that generates a composite dialog's ordered
  widget rendering + validation from `#[focus]`-style field annotations, or one that derives
  `TableEntry`/`Header` (the 11 columns) + `Widget` construction from annotated fields. Add them
  only where the boilerplate saved is significant, and keep generated paths fully qualified
  (`ferrowl_ui::…`) like the existing macro.

Deliberately avoided: a custom arg parser, a second serialization stack, a heavyweight logging
framework, and any crate flagged unmaintained.

## Architecture
- **`App` / `AppState`** (new `src/app.rs`): owns `Vec<Module>`, active tab index, a focus enum
  (`Table | Log | Command | Dialog`), the command-line buffer, the active overlay dialog
  (edit-register or setup-module), the general config, and the log-file base. Async loop:
  `loop { snap = collect_snapshot().await; terminal.draw(|f| render(snap)); select! { ev =
  events.next() => dispatch(ev), _ = tick => {} } }`, using crossterm `EventStream` (enable the
  `event-stream` feature) + a redraw `tokio::time::interval`.
- **Snapshot rendering**: before `draw`, `await` each visible module's `Memory`/`Log` read
  guards, decode rows + collect log lines into owned data, drop guards, then render. Removes the
  in-`render` `Memory` read currently in `view::main::Definition::values()`.
- **Event dispatch / focus** (vim-flavored; reuses the `Unhandled`-bubbling pattern in the
  current `main.rs:179-205`): `Table` is default focus; `gt`/`gT` switch module tabs; `Tab`
  toggles `Table`↔`Log` (log scroll via `j/k`, autoscroll at bottom); `:` enters `Command`;
  `Enter` on a row opens the edit dialog (`Dialog` focus); `Esc` cancels dialog / leaves command
  mode. Dialogs route events through their `Focus`-derived `HandleEvents`.
- **Lua simulation bridge**: a per-module `RegisterBridge` implementing `ferrowl_lua`'s
  `Read`/`Write` maps register **name** ↔ the module's `Memory` (decode/encode via the register's
  `ferrowl_reg::Register`, converting `ferrowl_reg::Value` ↔ `ValueType`). It's wrapped in
  `RegisterModule` and registered as the `C_Register` global. Because `mlua::Lua` is not `Send`,
  each module runs its update scripts on a **dedicated simulation thread** (created on
  `start`, joined on `stop`) that loops on the module's interval: it locks `Memory` with the
  tokio RwLock's `blocking_*` ops (safe off a runtime worker thread), runs `Context::refresh_all`
  / `call_all`, then releases. Cycle-based execution covers "run whenever a value is written"
  (the next tick recomputes).

## Execution process
Implement **one phase at a time** (Phase 0 → 8). After each phase, run the full check suite and
only advance when clean: `cargo build --workspace`, `cargo clippy --workspace --all-targets --
-D warnings`, `cargo test --workspace`, and `cargo fmt --all` (format). Fix every failure before
starting the next phase — the tree must compile, lint clean, and pass tests at every phase
boundary.

## Phases

**Phase 0 — Rename workspace to Ferrowl (prerequisite, do first).** `git mv` each `modbus-*/`
dir to `ferrowl-*/`; update `package.name` and all path deps in every `Cargo.toml`; update the
root workspace `members`; rewrite all `use modbus_*`/`modbus_*::` to `ferrowl_*`; rename the app
package `cli`→`ferrowl` (binary too). Critically, update the **hardcoded paths inside
`ferrowl-derive`’s generated code** (`ferrowl_ui::traits::…`, `ferrowl_ui::EventResult` →
`ferrowl_ui::…`). Update README, `.github/`, `.woodpecker/`, `configs/` references. Gate:
`cargo build --workspace` + `cargo clippy --workspace --all-targets` clean, tests pass.

**Phase 1 — Async shell.** Replace the 3-view demo loop with `App` + the async event loop in the
runtime; snapshot-based rendering; keep the single `TableView` rendering one module to prove the
loop. Cargo: crossterm `event-stream`, `tokio-stream`/`futures`, tokio
`rt-multi-thread`/`macros`/`fs`.

**Phase 2 — Composite main view.** Compose only from existing `ferrowl_ui` widgets
(table/input/selection/text/button) — no new widget types. Tab bar via ratatui's built-in `Tabs`;
scrollable log pane (new `view/log.rs`) renders the `ferrowl_log::Log` ring (`take_n`/`peak_n`)
through the existing `Text`/`Table` widget; command inputline (new `view/command.rs`) reuses
`InputField`. Lay out with `Layout::vertical`
([tabs=Length(3), table=Min, log=Length(N), cmd=Length(1)]). The register table **keeps the full
11-column `TableHeader`** already in `view::main` (Name, Comment, Slave ID, Address, Access, Kind,
Format, Length, Resolution, Value, Raw Value); only refactor `view::main::Definition` to hold the
register **metadata** and receive the decoded Value/Raw-Value from the snapshot instead of reading
`Memory` itself.

**Phase 3 — Config & CLI.** New `src/config/` serde structs: `AppConfig` (history_length,
default timing, log base), `DeviceConfig` (the `definitions` map: name → slave_id, read_code→
`Kind`, address, length, access, `type`→`Format`, resolution, optional `values[]` for
selections, `virtual`, and an optional **`update`** Lua-code string), `Session` (instances:
device file + transport + addr + role + name).
Load/save via `ferrowl_util::Converter` (TOML+JSON). New `src/cli.rs` (clap): `--config`,
repeatable `--module key=val` (+ a small key=val parser), `--session`. Build `Module`s from
CLI/session and auto-start. Config discovery in the priority order above.

**Phase 4 — Setup dialog.** New `src/dialog/setup.rs` (same `Widget`/`Focus` infra as the edit
dialogs): inputs for tab name, transport (TCP/RTU), ip+port or serial params, role
(client/server). Shown at startup when a module lacks info, and via `:e`/`:edit` for the current
tab. `:l`/`:load` reads a device file then opens this dialog to create the instance.

**Phase 4b — New-module dialog (`:n`/`:new`).** A dual-mode of the setup dialog: New mode adds
an **optional config-path** input that is validated live — empty creates an empty module,
otherwise the path must point at a loadable device config (errors shown in the Error box and
blocking confirm). On confirm it builds, auto-starts and appends a new module tab. The App stores
the `AppConfig` for runtime module creation; `handle_key` is async so the new module can start.

**Phase 5 — Command line.** New `src/command.rs`: parse + execute the command set. `:start`/
`:stop`/`:restart` drive `Instance` (in `instance/`); `:set <reg> <value>` encodes via
`Register::encode` then writes (`Memory` for servers, `Instance::send_command` for clients);
`:w`/`:l`/`:e`/`:log`/`:q` as specified.

**Phase 6 — Edit dialog wiring.** Give `EditInputDialog`/`EditSelectionDialog` a
`from_register(&Definition)` loader and `apply() -> Result<Definition, String>` (+ optional value
write reusing the Phase-5 path). Open pre-filled on `Enter`; open empty to create a register;
selection `Add` appends to the selection's value set (`SelectionState.values`). Pick
input-vs-selection dialog by whether the register has a named-value set. Both dialogs also gain a
multiline **`update`** (Lua) input field so the script is part of "edit any part of the register".

**Phase 7 — Per-module file logging.** Alongside the on-screen ring `Log`, add a file sink in
`Module::start`'s log callback (buffered append; filename = base + tab-name suffix). Configurable
via `--log` and `:log <file>`.

**Phase 8 — Lua per-module simulation.** Add `ferrowl-lua` as a dependency of the app crate.
Build each module's `ferrowl_lua::Context` from its `DeviceConfig` (`load_script(register_name,
update_code)` for every register that has an `update`), register the `C_Register` bridge
(`RegisterBridge` over the module's `Memory` + register table) plus `C_Time`/`C_Statics`, and
`enable_stdlib`. On `Module::start`, spawn the dedicated simulation thread that ticks on the
module interval and runs `refresh_all`/`call_all`; stop/join it on `Module::stop`. Lua/script
errors are surfaced into the module's `Log`. New: `src/lua.rs` (the `RegisterBridge` +
`Read`/`Write` impls + `Value`↔`ValueType` mapping + sim-thread driver).

## Critical files
- Phase 0 touches every crate: all `Cargo.toml`s, root `Cargo.toml`, every `*.rs` with a
  `modbus_*` import, `ferrowl-derive/src/lib.rs` (generated-code paths), README/CI/configs.
- New (in `ferrowl/`, formerly `ferrowl/`): `src/app.rs`, `src/cli.rs`, `src/command.rs`,
  `src/config/{mod,app,device,session}.rs`, `src/dialog/setup.rs`, `src/view/{tabs,log,command}.rs`,
  `src/lua.rs` (Lua bridge + sim thread).
- Modify: `src/main.rs` (slim async entry: parse CLI → load config → build `App` → run),
  `src/module.rs` (name + per-instance transport/role/addr + file logger + register list +
  snapshot accessor; build `Register`s/`Operation`s from `DeviceConfig`),
  `src/view/main.rs` (split metadata vs rendered row), `src/dialog/edit/{input,selection}.rs`
  (load/apply/value-write/Add), `Cargo.toml` (features/deps), `configs/` (device-type + general +
  session samples).

## Reuse (don't reinvent)
- `ferrowl_ui`: `Widget<S,W>`, `Table`/`Selection`/`InputField`/`Button`/`Text`, the `Focus`
  derive (`focus_next/previous` + routed `HandleEvents`), `EventResult` bubbling; ratatui `Tabs`.
  These cover every UI need — do not build new widgets; views/dialogs only compose them.
- `ferrowl_log::Log` ring (`take_n`/`peak_n`) for the log pane.
- `ferrowl_reg`: `Register::decode`/`encode`, `RegisterBuilder`, serde-ready `Access`/`Kind`/
  `Address`/`Format`.
- `ferrowl_net`: `Operation`, `Command`, `Config`, `Key`; the `instance/` layer (`start`/`stop`/
  `send_command`) abstracts tcp/rtu × client/server.
- `ferrowl_util::Converter` (TOML/JSON load/save), `Expect`.
- `ferrowl_mem`: `Memory` read/write, `Range`, `Type`, `Kind`.
- `ferrowl_lua`: `ContextBuilder`/`Context` (`load_script`, `refresh_all`/`call_all`),
  `RegisterModule`/`TimeModule`/`StaticsModule`, the `Read`/`Write` traits, `ValueType`.

## Verification
- After Phase 0: `cargo build --workspace`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all clean under the new names; binary is `ferrowl`.
- Unit tests: round-trip (de)serialize `AppConfig`/`DeviceConfig`/`Session` (TOML+JSON via
  `Converter`); `--module key=val` parser; command parser (each `:` command).
- End-to-end run with a `--session` of 2 server-sim modules on different ports + 1 client
  pointing at one. Confirm: `gt`/`gT` switch tabs; table shows live values; `Tab` focuses the log
  and `j/k` scroll it; `:start`/`:stop`/`:restart`/`:set`/`:w`/`:l`/`:e`/`:q` work; `Enter` opens
  the edit dialog pre-filled and apply reflects in the table / over modbus; selection `Add` and
  new-register creation work; setup dialog appears on `:l` and on missing-info startup; per-module
  log files are created with the tab-name suffix.
- Lua simulation: load an EVSE-style device config where `power` has an `update` that copies
  `setpoint`; start it as a server, write `setpoint` (via `:set` or an external client), and
  confirm `power` follows it on the next cycle in the table; a deliberately broken `update`
  surfaces its error in the module log.
