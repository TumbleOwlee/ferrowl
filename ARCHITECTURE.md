# Architecture

How ferrowl is put together: the workspace graph, what each crate owns, and how
the pieces interact at runtime. For *what the software must do* (behavior,
per-capability), see [`docs/specs/`](./docs/specs/); this file is the structural
map, not the spec.

## Workspace

Ferrowl is a Cargo workspace (resolver `"3"`, edition 2024) building one binary,
`ferrowl`, from twelve crates. All twelve are versioned in lockstep; none is
published independently.

<p align="center">
    <img src="./images/architecture.svg" alt="crate dependency graph">
</p>

| Crate | Responsibility |
|---|---|
| `ferrowl` | The binary. Event/redraw loop, tabs, views, dialogs, the `:` command line, session & device configuration, the `migrate` and `run` subcommands, the headless runner. |
| `ferrowl-ui` | Reusable [ratatui](https://ratatui.rs) building blocks: widgets and their state types, styling, alternate-screen handling, dialogs, tables. |
| `ferrowl-ui-derive` | Proc macros for the UI layer: `#[derive(TableEntry)]`, `#[derive(Overlay)]`, `#[derive(Focus)]` (keyboard focus cycling and event dispatch for views). |
| `ferrowl-lua-derive` | Proc macro `#[derive(Module)]`, which bridges a Rust host type into a Lua `C_*` module. |
| `ferrowl-codec` | Register descriptions (slave id, function code, address, access, format) and the codec between raw `u16` words and typed values. |
| `ferrowl-store` | In-memory model of a Modbus register space — access-checked value cells, shared as `Arc<RwLock<Memory>>`. |
| `ferrowl-modbus` | Modbus client and server tasks over TCP and RTU, on [tokio-modbus](https://github.com/slowtec/tokio-modbus). |
| `ferrowl-ocpp` | Version-generic OCPP engine (a `Version` trait over 1.6 / 2.0.1 / 2.1), Charging Station and CSMS roles, over JSON-on-WebSocket, wrapping [rust-ocpp](https://github.com/codelabsab/rust-ocpp). |
| `ferrowl-lua` | Embedded Lua 5.4 runtime ([mlua](https://github.com/mlua-rs/mlua)) exposing the `C_*` bridge modules to simulation scripts, in a restricted sandbox. |
| `ferrowl-syntax` | Syntax highlighting for the in-TUI code editor (Lua and JSON). |
| `ferrowl-ring` | Fixed-capacity ring buffer generic over the element type; backs each module's log pane. |
| `ferrowl-util` | Shared helpers: config (de)serialization, tracked tokio task spawning, small macros and traits. |

Grouped by concern: **Modbus** (`ferrowl-codec`, `ferrowl-store`, `ferrowl-modbus`),
**OCPP** (`ferrowl-ocpp`), **Lua** (`ferrowl-lua`, `ferrowl-lua-derive`),
**UI** (`ferrowl-ui`, `ferrowl-ui-derive`, `ferrowl-syntax`),
**Infra** (`ferrowl-ring`, `ferrowl-util`), and the **binary** (`ferrowl`) that ties
them together.

## Runtime data flow

The unit of work is a **module instance** — one configured Modbus or OCPP device,
shown as one tab. Each instance owns a shared, lock-protected state store:
`ferrowl-store::Memory` for Modbus, an `Arc<RwLock<S>>` state struct for OCPP.
Three concurrent parties touch that store:

1. **The network task** — a Modbus client polling a remote server (or a server
   answering incoming requests), or an OCPP connection (CS dialling a CSMS, or a
   CSMS accepting stations). It reads and writes the store.
2. **The Lua sim thread** — if scripts are configured, it reads and writes the same
   store through the typed `C_Register` / `C_OCPP` bridge, on a timer.
3. **The UI** — polls the store every redraw tick and renders it as a table.

## Concurrency model

The hot read/write path uses `parking_lot` synchronous locks, not `tokio::sync` —
deliberately, to avoid async-lock overhead and blocking-runtime pitfalls on a path
the UI hits every tick. Each Lua sim runs on its own dedicated OS thread, isolated
from the tokio runtime and the UI redraw loop, so a slow script cannot stall polling
or rendering. Modbus and OCPP are architecturally separate: there is no shared
`Instance<T>`-style lifecycle abstraction spanning both, and a pattern in one does
not necessarily transfer to the other.

The precise concurrency guarantees per module type are specified in
[`docs/specs/modbus/`](./docs/specs/modbus/) and
[`docs/specs/scripting/`](./docs/specs/scripting/).

## Where the internal-crate contracts live

The derive macros and highlighting crates have no capability spec of their own;
their observable contracts are specified where they are used:

- `#[derive(Module)]` (from `ferrowl-lua-derive`) → the `C_*` API it produces is
  specified in [`docs/specs/scripting/api-contract.md`](./docs/specs/scripting/api-contract.md).
- `#[derive(Focus/TableEntry/Overlay)]` (from `ferrowl-ui-derive`) and syntax
  highlighting (`ferrowl-syntax`) → their observable behavior is specified in
  [`docs/specs/tui/`](./docs/specs/tui/).
- `ferrowl-ring` and `ferrowl-util` are plain infrastructure; their contracts are
  their rustdoc.

## Map to the specs

| Area | Crates | Spec |
|---|---|---|
| Modbus | `ferrowl-codec`, `ferrowl-store`, `ferrowl-modbus` | [`docs/specs/modbus/`](./docs/specs/modbus/) |
| OCPP | `ferrowl-ocpp` | [`docs/specs/ocpp/`](./docs/specs/ocpp/) |
| Scripting | `ferrowl-lua`, `ferrowl-lua-derive` | [`docs/specs/scripting/`](./docs/specs/scripting/) |
| TUI | `ferrowl-ui`, `ferrowl-ui-derive`, `ferrowl-syntax` | [`docs/specs/tui/`](./docs/specs/tui/) |
| Config & session | `ferrowl-util`, parts of `ferrowl` | [`docs/specs/config-session/`](./docs/specs/config-session/) |
| CLI & headless | parts of `ferrowl` | [`docs/specs/cli-headless/`](./docs/specs/cli-headless/) |

