# Ferrowl — Product Requirements Document

**Status**: retroactive as-built spec, snapshot at v0.4.13 (2026-07-12).
**Scope**: the whole ferrowl workspace — TUI simulator for Modbus and OCPP devices.

This document describes what ferrowl *is*, for whom, and why it's shaped the way it is. It is deliberately not exhaustive — every crate's full API surface, exhaustive action/command/keybinding tables, and numeric constants live in [`docs/agents/`](./agents/prd.md), organized by domain (modbus, ocpp, lua, ui, infra). This file links to those instead of duplicating them, so there's one place to update per fact.

This is a **living document** — update it (and the relevant `docs/agents/*.md` file) in the same PR that changes the behavior it describes. See root `CONTRIBUTING.md`.

## 1. Overview

Ferrowl is a terminal (TUI) application, written in Rust, that simulates industrial/EV-charging network devices for testing and integration work:

- **Modbus** — client and server, over TCP and RTU (serial).
- **OCPP** (Open Charge Point Protocol) — Charging Station (client) and Central System/CSMS (server), versions 1.6, 2.0.1, and 2.1, over JSON-on-WebSocket.

Multiple independent module instances (any mix of Modbus/OCPP, client/server, any version) run side by side as tabs in one process, each with its own live register/state table, message log, and optional Lua-scripted simulation behavior. Configurations and full multi-module sessions can be saved to and loaded from TOML/JSON files, so a test rig can be reproduced exactly on demand.

It targets engineers who need a protocol-accurate stand-in for real hardware — an EVSE, a meter, a CSMS — without a GUI environment: CI runners, SSH sessions, headless test rigs.

## 2. Goals

- Simulate Modbus servers/clients and OCPP charging stations/CSMSes with enough protocol fidelity to stand in for real devices in integration testing.
- Make register/state manipulation and inspection fast and visual, even in a terminal.
- Let simulated device *behavior* (not just static values) be scripted, so a session can model something dynamic — a charging session ramping power, a meter drifting, a CS responding to remote commands.
- Make setups reproducible: everything material to a test rig's behavior is expressible in a config file, diffable and version-controllable.
- Run the same simulation logic interactively (TUI) or headlessly (`ferrowl run`, for CI).

## 3. Non-goals

- Not a GUI application — no plans for one; the README explicitly points GUI-preferring users elsewhere (e.g. QModbus).
- Not a conformance/certification test suite for Modbus or OCPP — it does not claim to validate a real device against the spec, only to simulate one.
- Not a general-purpose scripting platform — the Lua surface is deliberately narrow (register/OCPP-state access, logging, time), not a sandboxed multi-tenant execution environment (see [Known limitations](#6-known-limitations--out-of-scope), execution has no timeout or memory ceiling).
- Not a persistence/database layer — all simulated state is in-memory for the life of the process; only configuration (not live register/transaction state) survives a restart.

## 4. Users & use cases

Primary user: an engineer integrating or testing software that talks Modbus or OCPP — e.g. building a charge-point management system and needing a fake Charging Station to drive it, or writing a SCADA integration and needing a fake Modbus meter.

Representative use cases:
1. **EVSE simulation** — bring up a fake Charging Station (any of 1.6/2.0.1/2.1) that connects to a real or test CSMS, and drive it through boot/authorize/start-transaction/meter-values/stop-transaction via Lua scripting or manual action dialogs.
2. **CSMS simulation** — bring up a fake Central System that accepts real charge points, to test a CS implementation's OCPP compliance without a live backend.
3. **Modbus device simulation** — stand in for a meter/PLC/inverter as a Modbus TCP or RTU server, with registers seeded from a device profile and optionally ticking via Lua.
4. **Modbus polling client** — poll a real or simulated Modbus server, to inspect/exercise it interactively.
5. **CI-driven scenario runs** — `ferrowl run --session ... --duration 60 --exit-on-error`, headless, so a session's Lua scripts can assert expected behavior (`C_Test:Assert`) and fail the build on error.
6. **Config migration** — `ferrowl migrate` to bring forward a pre-v0.4.0 (`modbus-cli-rs`) config to the current format.

## 5. Architecture

Ferrowl is a Cargo workspace of 12 crates building one `ferrowl` binary. See `README.md`'s architecture diagram (`images/architecture.svg`) for the dependency graph. Grouped by concern:

| Group | Crates | What it owns |
|---|---|---|
| Modbus | `ferrowl-codec`, `ferrowl-store`, `ferrowl-modbus` | Register description/codec, in-memory register space, client/server network tasks over TCP+RTU |
| OCPP | `ferrowl-ocpp` | Version-generic (1.6/2.0.1/2.1) CS + CSMS engine over JSON-on-WebSocket |
| Lua | `ferrowl-lua`, `ferrowl-lua-derive` | Embedded Lua 5.4 runtime exposing register/OCPP/log/time bridges to sim scripts |
| UI | `ferrowl-ui`, `ferrowl-ui-derive`, `ferrowl-syntax` | Reusable ratatui widgets, focus/table/overlay derive macros, Lua/JSON syntax highlighting |
| Infra | `ferrowl-ring`, `ferrowl-util` | Fixed-capacity log ring buffer, config (de)serialization + task-spawning helpers |
| Binary | `ferrowl` | Ties everything together: event/redraw loop, tabs, views, dialogs, `:` commands, session & device config, CLI, `migrate` subcommand |

**Runtime data flow**, per module instance: the network task (Modbus client/server, or OCPP CS/CSMS connection) reads and writes a shared, lock-protected state store (`ferrowl-store::Memory` for Modbus, an `Arc<RwLock<S>>` state struct for OCPP). A Lua sim thread, if scripts are configured, reads and writes that same store through a typed bridge (`C_Register`/`C_OCPP`). The UI polls it every redraw tick and renders it as a table. All three — network, script, UI — can touch the state concurrently; the concurrency model differs by module type and is detailed in `docs/agents/modbus.md` §2.6 and `docs/agents/lua.md` §3.

Full detail (every type, method, numeric constant) is in the domain files under `docs/agents/`; start there for anything below the architecture level.

## 6. Feature surface (summary — see linked domain docs for exhaustive detail)

### 6.1 Modbus
Client and server, TCP and RTU. Reads/writes all four Modbus register tables (coils, discrete inputs, holding registers, input registers). Registers are typed (13 numeric/ASCII formats, both endians, bit-fields, display-scaling) via `ferrowl-codec`. Auto-reconnect with exponential backoff on the client side. → [`docs/agents/modbus.md`](./agents/modbus.md)

### 6.2 OCPP
Charging Station and Central System/CSMS roles, OCPP 1.6 (28 actions), 2.0.1 (64 actions), and 2.1 (90 actions) simultaneously supported. TLS (server + optional mutual TLS) and HTTP Basic Auth. Per-action typed send dialogs where the payload is flat, raw-JSON editor where it's inherently nested. → [`docs/agents/ocpp.md`](./agents/ocpp.md)

### 6.3 Lua scripting
Every module (and the session as a whole) can run Lua 5.4 scripts on a timer, reading/writing register or OCPP state and triggering OCPP actions, via a small fixed API (`C_Register`, `C_OCPP`, `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`). A vim-modal in-TUI code editor with syntax highlighting is the primary authoring surface. → [`docs/agents/lua.md`](./agents/lua.md)

### 6.4 TUI / operator surface
Tabbed multi-module UI, live-updating tables, modal dialogs for setup/editing/actions, a `:`-command line (vim-style), and both vim-style and arrow-key navigation. A headless mode (`ferrowl run`) runs the same module + Lua-sim machinery without a terminal, for CI. → [`docs/agents/ui.md`](./agents/ui.md)

### 6.5 Configuration & session management
Every module instance is described by a device config file plus a session file that lists instances and their endpoints; both are TOML or JSON. Sessions can be saved from the running TUI (`:write`) and reloaded exactly. A `migrate` subcommand upgrades pre-v0.4.0 configs. → [`docs/agents/infra.md`](./agents/infra.md) §4–5

## 7. Non-functional

- **Platforms**: Linux and Windows prebuilt nightly binaries (built via cross-compilation to `x86_64-pc-windows-gnu`); developed and CI'd primarily on Linux (`rust:latest` container). No macOS binaries are built by CI, though nothing in the stack is Linux-specific beyond the build pipeline.
- **Performance posture**: no explicit performance targets or benchmarks exist in the repo. The design leans on `parking_lot` sync locks (not async) for the hot read/write path specifically to avoid `tokio::sync` overhead and blocking-runtime pitfalls (`docs/agents/modbus.md` §2.6). Lua scripts run on dedicated OS threads, isolated from the tokio runtime and the UI redraw loop, so a slow script cannot stall polling or rendering — but see the timeout caveat below.
- **Reliability**: Modbus clients auto-reconnect with exponential backoff (1s–30s). OCPP connections do not auto-reconnect at the crate level as of this snapshot — reconnection is an operator action (`:restart`). A Lua script error never crashes its host module; errors are logged and the script loop continues.
- **Security posture**: OCPP supports TLS (including mutual TLS) and HTTP Basic Auth; Modbus has no transport security (matches the protocol's own lack of one). Lua scripts run in mlua's "safe" stdlib subset (no `io`/`os.execute`/`debug`), but **without any CPU-time or memory ceiling** — see [Known limitations](#6-known-limitations--out-of-scope).
- **Testing**: workspace-wide `cargo test`, unit tests colocated with code (`#[cfg(test)] mod tests`, `ut_*` naming — see `docs/agents/infra.md` §6). CI (Woodpecker) runs `cargo check` + `cargo test` on every push; a tag-triggered nightly workflow additionally builds and publishes release binaries for Linux and Windows. `lefthook` enforces `cargo fmt --check` and `cargo clippy -- -D warnings` pre-commit.
- **Versioning**: all 12 workspace crates are versioned in lockstep (currently `0.4.13`); no crate is published independently.

## 8. Known limitations / out of scope

These are real, current gaps — not roadmap items — documented so they're not mistaken for oversights during review:

- **No Lua execution ceiling**: sim scripts have no instruction-count, wall-clock, or memory limit (`docs/agents/lua.md` §5). An infinite loop in a script hangs that module's dedicated sim thread indefinitely (it does not, however, block the UI or other modules, since each sim runs on its own OS thread).
- **No max-connectors / max-registers-per-request constant**: both are practically unbounded (register/connector lists are plain `Vec`s; Modbus request size is bounded only by the wire protocol's own `u16` field width).
- **`ferrowl-ocpp/README.md` is out of date**: it omits OCPP 2.1 entirely, though the crate ships and fully implements it by default. Fixing that README is a known, not-yet-done cleanup.
- **RTU `Config` / `clap` flattening bug**: a short-flag collision (`-s` claimed by both `slave` and `stop_bits`, `-d` by both `data_bits` and `delay_ms`) means `ferrowl_modbus::rtu::Config` cannot currently be flattened directly into a `clap::Parser` (`docs/agents/modbus.md` §3.1) — pre-existing, unfixed.
- **`ferrowl-util`'s tracked-task registry (`spawn_detach`/`join_all`) is unused** by the app crate as of this snapshot; the mechanism exists but nothing in `ferrowl/src` calls it (`docs/agents/infra.md` §2). Don't assume it's load-bearing for shutdown correctness.
- **Session `version` field is informational only** — no migration logic currently branches on it, despite the field's doc comment saying it "enables future compatibility shims."
- **No macOS CI builds.**
- **OCPP has no crate-level auto-reconnect** (contrast with Modbus's client backoff) — a dropped OCPP connection stays dropped until an operator issues `:restart`.

## 9. Glossary

| Term | Meaning |
|---|---|
| CS | Charging Station — the OCPP client role (the physical/simulated charge point) |
| CSMS | Charging Station Management System — the OCPP server role (also called Central System in OCPP 1.6) |
| EVSE | Electric Vehicle Supply Equipment — a charging connector/outlet, addressable in OCPP 2.0.1/2.1 |
| Coil / Discrete Input / Holding Register / Input Register | The four Modbus register address spaces (1-bit R/W, 1-bit RO, 16-bit R/W, 16-bit RO respectively) |
| Slave id / Unit id | Modbus device address on a shared bus/connection |
| RTU | Modbus over serial (as opposed to TCP) |
| Module | One configured Modbus or OCPP instance, shown as one tab in the TUI |
| Session | A saved file listing multiple module instances plus session-level Lua scripts, restorable in one command |
| Sim / sim script | A Lua script attached to a module (or the session) that runs on a timer to simulate device behavior |
| Virtual register | A Modbus register with no fixed wire address — script-only, never sent over the wire |
| `C_*` modules | The fixed set of Lua global tables (`C_Register`, `C_OCPP`, `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`) exposed to sim scripts |
| Headless mode | `ferrowl run` — runs modules and Lua sims without a terminal UI, for CI |

## 10. Reference index

| Need | Where |
|---|---|
| Every Modbus function code, format, config field, numeric constant | [`docs/agents/modbus.md`](./agents/modbus.md) |
| Every OCPP action per version, config field, TLS/auth option | [`docs/agents/ocpp.md`](./agents/ocpp.md) |
| Every Lua API method, sim execution/error model | [`docs/agents/lua.md`](./agents/lua.md) |
| Every `:` command, keybinding, CLI flag, dialog | [`docs/agents/ui.md`](./agents/ui.md) |
| `Instance<T>` lifecycle, `migrate` internals, session bootstrap, CI/build config | [`docs/agents/infra.md`](./agents/infra.md) |
| High-level architecture diagram | `images/architecture.svg` (linked from root `README.md`) |
| Contribution workflow, pre-submit checklist | root `CONTRIBUTING.md` |
