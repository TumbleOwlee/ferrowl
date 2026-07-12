# Ferrowl — Agent PRD Index

Dense, agent-oriented spec of ferrowl v0.4.13. Load only the domain file(s) relevant to your task — each is self-contained and cites `file:line` for nontrivial claims. For narrative/rationale framing (goals, users, non-functional), see `../prd.md`; that file links back here for exhaustive detail instead of duplicating it.

This index and the domain files are **living docs** — update them in the same PR that changes the behavior they describe (see root `CONTRIBUTING.md`).

## Domain files

| File | Covers | Crates |
|---|---|---|
| [`modbus.md`](./modbus.md) | Register codec, in-memory register store, Modbus client/server (TCP+RTU), app-level device/session config | `ferrowl-codec`, `ferrowl-store`, `ferrowl-modbus` |
| [`ocpp.md`](./ocpp.md) | Version-generic OCPP engine, CS/CSMS roles, exhaustive action tables (1.6/2.0.1/2.1), app-level module config, typed vs. raw-JSON send dialogs | `ferrowl-ocpp` |
| [`lua.md`](./lua.md) | `#[derive(Module)]`, full `C_*` Lua API surface, sim thread execution model, script authoring/storage | `ferrowl-lua`, `ferrowl-lua-derive` |
| [`ui.md`](./ui.md) | Reusable widgets, `#[derive(Focus/TableEntry/Overlay)]`, syntax highlighting, app tabs/views/dialogs, exhaustive `:` commands, keybindings, CLI flags | `ferrowl-ui`, `ferrowl-ui-derive`, `ferrowl-syntax` |
| [`infra.md`](./infra.md) | Ring buffer, config (de)serialization helpers, tracked-task registry, `Instance<T>` lifecycle, `migrate` subcommand, session bootstrap, build/CI/versioning | `ferrowl-ring`, `ferrowl-util` |

## Quick facts an agent usually needs first

- Binary crate: `ferrowl` (`ferrowl/src/`). Workspace resolver `"3"`, 12 members, all versioned in lockstep (currently `0.4.13`).
- Two module types exist per tab: **Modbus** (client/server, TCP/RTU) and **OCPP** (Charging Station/CSMS, versions 1.6/2.0.1/2.1, WS/WSS). No third module type as of this snapshot.
- `ferrowl-ocpp/README.md` is stale re: v2.1 — the crate ships and fully implements v2.1 by default; trust `docs/agents/ocpp.md` and the code.
- Unit tests sit in `#[cfg(test)] mod tests` at the bottom of the file under test — that's where to look for executable examples of any API in this index. The `ut_*` prefix is the convention but is **not** universal (24% of in-`src` tests lack it; the UI/derive crates mostly do). Integration tests live in `tests/` dirs. See `infra.md` §6.
- Config files are TOML or JSON (extension-driven), never YAML or anything else.
- Lua is the only scripting surface; scripts are stored inline in config files (not external `.lua` files) and have no execution timeout/memory limit (`lua.md` §5).
