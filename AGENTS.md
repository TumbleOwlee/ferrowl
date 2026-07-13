# AGENTS.md

Router for AI coding agents working in this repo. Read this first; it points to
everything else.

## What this repo is

Ferrowl — a Rust TUI simulator for Modbus (client/server, TCP/RTU) and OCPP
(Charging Station/CSMS, versions 1.6/2.0.1/2.1) devices. A Cargo workspace of 12
crates building one `ferrowl` binary. Product framing: [`PRD.md`](./PRD.md).
Structure and crate map: [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Spec-driven — read this before you change behavior

`docs/specs/` is the **authoritative** specification: the code is expected to
conform to it, not the other way around. Before you edit code in an area, read that
area's `requirements.md`. If your change alters behavior the spec describes, update
the spec **in the same commit** — a behavior change with no spec change is
incomplete. If the code and the spec already disagree, that is a defect in one of
them: resolve it or flag it, don't work around it.

Specs contain no `file:line` pointers by design — locate code with your own search
tools. Requirements have stable IDs (`MB-R-*`, `OC-R-*`, …); reference them in
commits and PRs.

## Where to look for task X

| Task touches | Read |
|---|---|
| Modbus register codec, store, client/server (TCP/RTU) | [`docs/specs/modbus/`](./docs/specs/modbus/) |
| OCPP actions, CS/CSMS engine, versions 1.6/2.0.1/2.1, TLS/auth | [`docs/specs/ocpp/`](./docs/specs/ocpp/) |
| Lua scripting (`C_*` API, sim threads, sandbox) | [`docs/specs/scripting/`](./docs/specs/scripting/) |
| TUI widgets, dialogs, `:` commands, keybindings, code editor | [`docs/specs/tui/`](./docs/specs/tui/) |
| Config/session file format, save/load, `migrate` | [`docs/specs/config-session/`](./docs/specs/config-session/) |
| CLI flags, `ferrowl run` headless, exit codes | [`docs/specs/cli-headless/`](./docs/specs/cli-headless/) |
| Platforms, performance, security, versioning | [`docs/specs/non-functional-requirements.md`](./docs/specs/non-functional-requirements.md) |
| Crate graph, data flow, concurrency model | [`ARCHITECTURE.md`](./ARCHITECTURE.md) |
| Contribution workflow, conventions | [`CONTRIBUTING.md`](./CONTRIBUTING.md) |

Each area's `edge-cases.md` records its **known limitations** — behavior that is
ugly but intentional. Check it before "fixing" something that looks wrong.

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

Run these before considering work done — `lefthook` enforces `fmt --check` and
`clippy -D warnings` pre-commit; CI runs `check` + `test` on every push.

Dev loop: `cargo run --release -- --demo` (built-in demo tabs, no config needed) or
`cargo build --profile fastrel` for faster iterative builds (opt-level 1).

## Conventions

- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file under test,
  function names prefixed `ut_`. For integration tests, the function names are 
  prefixed with `it_`  (notably in `ferrowl-ui` and much of `ferrowl`). Integration
  tests belong in each crate's `tests/`.
- All 12 workspace crates are versioned in lockstep. Don't bump one independently.
- Config files are TOML or JSON only (extension-driven), never YAML.
- Rust edition 2024, stable toolchain (`rust-toolchain.toml`).

## Scope boundaries — check with the user before

- **Expanding the Lua `C_*` API.** The surface (`C_Register`, `C_OCPP`, `C_Time`,
  `C_Log`, `C_Test`, `C_Module`, `C_Statics`) is deliberately small and fixed.
  Adding a module or method is a design decision, not a mechanical addition.
- **Bridging Modbus and OCPP.** They are architecturally separate — no shared
  lifecycle abstraction spans both. Don't assume a fix/pattern in one applies to
  the other without checking both specs.
