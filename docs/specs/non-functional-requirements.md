# Non-Functional Requirements

Cross-cutting properties that hold across every capability area. Per-area
functional behavior is in each area's `requirements.md`; this file covers the
qualities that are not owned by any single area.

IDs are stable and append-only (`NF-R-nnn`). See [`README.md`](./README.md).

## Platforms

**NF-R-001** — Linux and Windows prebuilt binaries shall be produced by the
nightly pipeline. Windows is built by cross-compilation to
`x86_64-pc-windows-gnu`; development and CI run primarily on Linux.

**NF-R-002** — No macOS binaries are built by CI. Nothing in the stack shall be
Linux-specific beyond the build pipeline itself.

**NF-R-003** — The toolchain shall be stable Rust, edition 2024, pinned by
`rust-toolchain.toml` (patch version unpinned).

## Performance posture

**NF-R-010** — No explicit performance targets or benchmarks are asserted. The
design shall keep the hot register read/write path on `parking_lot` synchronous
locks rather than `tokio::sync`, to avoid async-lock overhead on a path the UI
touches every redraw tick.

**NF-R-011** — Each Lua sim shall run on its own dedicated OS thread, isolated from
the tokio runtime and the UI redraw loop, so a slow script cannot stall polling or
rendering. (The absence of an execution ceiling is a known limitation — see
[`scripting/edge-cases.md`](./scripting/edge-cases.md).)

## Reliability

**NF-R-020** — A Modbus client shall auto-reconnect with exponential backoff
bounded to 1s–30s. (Specified in [`modbus/`](./modbus/).)

**NF-R-021** — An OCPP connection shall not auto-reconnect at the crate level; a
dropped connection stays dropped until an operator issues `:restart`. (Specified in
[`ocpp/`](./ocpp/).)

**NF-R-022** — A Lua script error shall never crash its host module: the error is
logged and the script loop continues.

## Security posture

**NF-R-030** — OCPP shall support TLS (including mutual TLS) and HTTP Basic Auth.
Modbus has no transport security, matching the protocol's own lack of one.

**NF-R-031** — Lua sim scripts shall run in a restricted sandbox: only the
pure-computation standard libraries (`string`, `table`, `math`, `utf8`,
`coroutine`) are reachable; `io`, `os`, `package`, `debug`, FFI, and the base
dynamic-code loaders (`load`, `loadfile`, `dofile`, `require`) are not. A script
therefore has no access to the host filesystem, shell, environment, or dynamic
code loading. (Specified in [`scripting/`](./scripting/).) There is no CPU-time or
memory ceiling — a separate, known limitation.

## Versioning & testing

**NF-R-040** — All twelve workspace crates shall be versioned in lockstep; no crate
is published independently.

**NF-R-041** — Unit tests shall be colocated with the code under test
(`#[cfg(test)] mod tests`, `ut_*` naming where practical); integration tests live
in each crate's `tests/`. CI shall run `cargo check` + `cargo test` on every push;
a tag-triggered nightly workflow additionally builds and publishes release
binaries. `lefthook` shall enforce `cargo fmt --check` and `cargo clippy -D
warnings` pre-commit.
