# Contributing to Ferrowl

Thanks for your interest in contributing! This document covers the essentials to get you productive quickly.

## Setup

Ferrowl is written in Rust (stable toolchain, pinned via `rust-toolchain.toml`). Install the toolchain via [rustup.rs](https://rustup.rs/), then:

```sh
git clone <your-fork>
cd ferrowl
cargo build --release
```

Run the app during development with `cargo run --release -- --demo` (starts a demo server) or see `--help` for all runtime options.

## Project Layout

The repository is a Cargo workspace building the `ferrowl` binary. See
[`ARCHITECTURE.md`](./ARCHITECTURE.md) for the crate dependency graph and each
crate's responsibility, and [`PRD.md`](./PRD.md) for the product framing.

Ferrowl is **spec-driven**: [`docs/specs/`](./docs/specs/) is the authoritative
specification of what the software must do, split by capability area. The code is
expected to conform to it. Before changing behavior, read the relevant area's
`requirements.md`.

## Before Submitting

Please make sure the following pass locally:

```sh
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo check
cargo test --workspace
```

CI runs all four as separate steps of the `check` pipeline — on every push **and every pull request** — so anything the pre-commit hook would reject is rejected by CI too. A `nightly` workflow (only run on `main`) builds the prebuilt executables published on the Release page.

## Pull Requests

- Branch off `main` and open your PR against `main`.
- Keep PRs focused — one feature or fix per PR.
- Add or update tests for behavior changes; the existing unit tests live in `#[cfg(test)]` modules next to the code (`ut_*` naming).
- **Update the spec in the same PR.** When you change behavior, update the relevant `docs/specs/<area>/` file(s) — they are the authoritative source, not a one-time snapshot. New requirements get a fresh, appended ID (never renumber or reuse). A behavior change with no spec change is incomplete.
- Update the README when you change user-facing commands, keybindings, configuration fields, or the Lua API.

## Reporting Issues

Open a GitHub issue with steps to reproduce, the ferrowl version (or commit), and your platform. For TUI rendering issues, the terminal emulator and size are helpful too.
