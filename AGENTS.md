# AGENTS.md

Instructions for AI coding agents working in this repo. Read this first; it's short and points to dense reference docs for everything else.

## What this repo is

Ferrowl — Rust TUI simulator for Modbus (client/server, TCP/RTU) and OCPP (Charging Station/CSMS, versions 1.6/2.0.1/2.1) devices. Cargo workspace, 12 crates, one `ferrowl` binary. Full narrative overview: [`docs/prd.md`](./docs/prd.md).

## Crates

| Crate | Responsibility |
|---|---|
| `ferrowl` | Binary. App/tabs/dialogs, `Instance<T>` lifecycle, module config, `migrate` subcommand, CLI. |
| `ferrowl-codec` | Register definition, `Kind` (4 Modbus tables), 13 data `Format`s, encode/decode. |
| `ferrowl-store` | In-memory register store (`Memory<K>`), address ranges. |
| `ferrowl-modbus` | Modbus client/server cores, TCP + RTU transports. |
| `ferrowl-ocpp` | Version-generic OCPP engine, CS + CSMS roles, action tables 1.6/2.0.1/2.1. |
| `ferrowl-lua` | Lua runtime, `C_*` API surface, sim thread execution. |
| `ferrowl-lua-derive` | `#[derive(Module)]` for exposing Rust types to Lua. |
| `ferrowl-ui` | Reusable ratatui widgets, dialogs, tables. |
| `ferrowl-ui-derive` | `#[derive(Focus/TableEntry/Overlay)]`. |
| `ferrowl-syntax` | Syntax highlighting (Lua, JSON). |
| `ferrowl-ring` | Fixed-capacity ring buffer (log/message history). |
| `ferrowl-util` | Config (de)serialization helpers, tracked-task registry. |

Dependency graph: [`README.md` §Architecture](./README.md#architecture).

## Where to look before changing code

Don't re-derive facts by grepping cold — the domain docs already have them, with `file:line` citations:

| Task touches | Read first |
|---|---|
| Modbus register codec, store, client/server (TCP/RTU) | [`docs/agents/modbus.md`](./docs/agents/modbus.md) |
| OCPP actions, CS/CSMS engine, versions 1.6/2.0.1/2.1 | [`docs/agents/ocpp.md`](./docs/agents/ocpp.md) |
| Lua scripting (`C_*` API, sim threads, script storage) | [`docs/agents/lua.md`](./docs/agents/lua.md) |
| TUI widgets, dialogs, `:` commands, keybindings, CLI flags | [`docs/agents/ui.md`](./docs/agents/ui.md) |
| `Instance<T>` lifecycle, `migrate`, session bootstrap, ring buffer, config helpers, CI/build | [`docs/agents/infra.md`](./docs/agents/infra.md) |

Index/router for the above: [`docs/agents/prd.md`](./docs/agents/prd.md). Human-facing goals/users/non-functional framing: [`docs/prd.md`](./docs/prd.md).

These are **living docs**. If a change alters behavior a doc describes, update that doc in the same PR — don't leave it to drift (same rule as `CONTRIBUTING.md`).

## Build / test / lint

```sh
cargo check
cargo test --workspace
cargo clippy --workspace       # CI and lefthook run with -D warnings
cargo fmt --check
```

Narrow the loop while iterating — don't run the whole workspace for one test:

```sh
cargo test -p ferrowl-modbus              # one crate
cargo test -p ferrowl-codec ut_decode     # one test (unit tests are named ut_*)
cargo check -p ferrowl-ocpp               # typecheck one crate
```

Run these before considering work done — `lefthook` enforces `fmt --check` and `clippy -D warnings` pre-commit; CI (`.woodpecker/check.yml`) runs `check` + `test` on every push.

Dev loop: `cargo run --release -- --demo` (8 built-in demo tabs, no config files needed) or `cargo build --profile fastrel` for faster iterative builds (opt-level 1).

## Conventions

- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the same file as the code under test, function names prefixed `ut_`. Follow this for new **unit** tests.
  - The rule is not retroactively true — 245 of 1009 in-`src` test fns lack the `ut_` prefix. It holds 100% in `ferrowl-codec`/`-store`/`-modbus`/`-ocpp`/`-lua`/`-syntax`/`-util`/`-ring`; it does **not** in `ferrowl-ui` (116/118 unprefixed), `ferrowl-ui-derive`, `ferrowl-lua-derive`, or `ferrowl` (105/534). Don't rely on `ut_` when grepping for existing tests.
  - Integration tests **do** belong in `tests/` (13 files today: loopback, render, derive-macro, headless). Inline-only applies to unit tests. See `docs/agents/infra.md` §6.
- All 12 workspace crates are versioned in lockstep (`Cargo.toml` `version` field identical everywhere). Don't bump one crate's version independently.
- Config files are TOML or JSON only (file-extension-driven), never YAML.
- Update `README.md` when changing commands, keybindings, config fields, or the Lua API — same rule as the `docs/agents/*.md` files above, see `CONTRIBUTING.md`.
- Rust edition 2024, stable toolchain (`rust-toolchain.toml`, unpinned patch version).

## Known sharp edges (see `docs/prd.md` §8 for the full list)

- Lua sim scripts have **no execution timeout or memory ceiling** — an infinite loop in a script hangs only its own dedicated OS thread, not the UI or other modules, but it never self-terminates.
- `ferrowl-ocpp/README.md` is stale — it omits OCPP 2.1 even though the crate ships and fully implements it by default. Trust `docs/agents/ocpp.md` and the code over that README.
- `ferrowl_modbus::rtu::Config` can't be flattened into a `clap::Parser` (pre-existing short-flag collision: `-s` and `-d` both double-booked). Don't "fix" this incidentally as a side effect of unrelated work without flagging it explicitly — it's a known, deliberately-untouched issue.
- `ferrowl-util`'s tracked-task registry (`spawn_detach`/`join_all`) is unused by the app crate — don't assume it's load-bearing for shutdown correctness.

## Scope boundaries

- Modbus and OCPP are architecturally separate (no shared `Instance<T>`-style lifecycle abstraction spans both) — don't assume a fix/pattern in one applies to the other without checking `docs/agents/modbus.md` vs `docs/agents/ocpp.md`.
- The Lua API surface (`C_Register`, `C_OCPP`, `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`) is deliberately small and fixed — adding a new `C_*` module or method is a design decision, not a mechanical addition; check with the user before expanding it.
