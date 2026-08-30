# Non-Functional Requirements

Cross-cutting properties holding across every area. Per-area behavior: each area's `requirements.md`.

IDs stable, append-only (`NF-R-nnn`). See [`README.md`](./README.md).

## Platforms

**NF-R-001** — Linux and Windows prebuilt binaries shall be produced by the nightly pipeline. Windows is cross-compiled to `x86_64-pc-windows-gnu`; development and CI run primarily on Linux.

**NF-R-002** — No macOS binaries are built by CI. Nothing in the stack shall be Linux-specific beyond the build pipeline itself.

**NF-R-003** — The toolchain shall be stable Rust, edition 2024, pinned by `rust-toolchain.toml` (patch version unpinned).

## Performance posture

**NF-R-010** — No explicit performance targets or benchmarks are asserted. The hot register read/write path shall stay on `parking_lot` synchronous locks, not `tokio::sync`, avoiding async-lock overhead on a path the UI touches every redraw tick.

**NF-R-011** — Each Lua sim shall run on its own dedicated OS thread, isolated from the tokio runtime and the UI redraw loop, so a slow script cannot stall polling or rendering. (No execution ceiling is a known limitation — [`scripting/edge-cases.md`](./scripting/edge-cases.md).)

## Reliability

**NF-R-020** — A Modbus client shall auto-reconnect with exponential backoff bounded to 1s–30s. (Specified in [`modbus/`](./modbus/).)

**NF-R-021** — An OCPP connection (CS or CSMS) shall auto-reconnect using the same bounded exponential-backoff policy as the Modbus client (MB-R-051), gated by the CS's `reconnect` config field (default enabled) and, for a CSMS, applied unconditionally to a failed listener bind. (Specified in [`ocpp/`](./ocpp/), OC-R-048, OC-R-083.)

**NF-R-022** — A Lua script error shall never crash its host module. (Specified in [`scripting/`](./scripting/), SC-R-032.)

## Security posture

**NF-R-030** — OCPP shall support TLS (including mutual TLS) and HTTP Basic Auth. Modbus/TCP shall optionally support TLS, including mutual TLS, via an opt-in `tls` config field (MB-R-104–MB-R-111). Modbus RTU has no transport security, matching the protocol.

**NF-R-031** — Lua sim scripts shall run in a restricted sandbox with no access to host filesystem, shell, environment, or dynamic code loading. (Specified in [`scripting/`](./scripting/), SC-R-006.) Wall-clock execution time is capped (SC-R-034); no memory ceiling — a known limitation.

**NF-R-032** — A credential comparison during peer authentication (e.g. OCPP CSMS Basic Auth) shall run in constant time with respect to the secret, so a wrong guess leaks no timing signal about where it first diverges.

## Path handling

**NF-R-042** — Any user-supplied filesystem path — CLI flag argument, path-valued config/session/device field, TUI dialog path field — shall have a leading `~` expanded to the current user's home directory before it is opened, read, written, or checked for existence: bare `~` = home directory, `~/rest` = `<home>/rest`. A path not starting with `~` (including `~otheruser/...`, unsupported — no portable std API resolves another user's home) passes through unchanged. Home directory undeterminable → passed through unchanged, no error. Expansion is performed once by a single shared resolver, applied at every filesystem-touching call site so every path-based feature resolves `~` identically: config/session/device config files (`ferrowl-util::convert::Converter`), CLI `--session`/`--device`/`--module`/`--log-file`, per-module log files, Modbus/OCPP TLS cert/key/CA files (including the setup dialogs' path-existence validation, so a typed `~/...` path validates the same way it later loads).

## Versioning & testing

**NF-R-040** — All workspace crates shall be versioned in lockstep; no crate is published independently.

**NF-R-041** — Unit tests shall be colocated with the code under test (`#[cfg(test)] mod tests`, `ut_*` naming where practical); integration tests in each crate's `tests/`. CI shall run `cargo check` + `cargo test` on every push; a tag-triggered nightly workflow additionally builds and publishes release binaries. `lefthook` shall enforce `cargo fmt --check` and `cargo clippy -D warnings` pre-commit.
