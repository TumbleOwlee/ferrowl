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

The repository is a Cargo workspace building the `ferrowl` binary. See the [Architecture section](./README.md#architecture) of the README for the crate dependency graph and each crate's responsibility.

## Before Submitting

Please make sure the following pass locally:

```sh
cargo check
cargo test --workspace
cargo clippy --workspace
cargo fmt --check
```

CI runs `cargo check` and `cargo test` on every push (`check` workflow); a `nightly` workflow (only run on `main`) builds the prebuilt executables published on the Release page.

## Pull Requests

- Branch off `main` and open your PR against `main`.
- Keep PRs focused — one feature or fix per PR.
- Add or update tests for behavior changes; the existing unit tests live in `#[cfg(test)]` modules next to the code (`ut_*` naming).
- Update the README when you change commands, keybindings, configuration fields or the Lua API.
- Update `docs/prd.md` and the relevant `docs/agents/*.md` file(s) when you change behavior they document (see `docs/agents/prd.md` for which file covers which crate) — they're living docs, not a one-time snapshot.

## Reporting Issues

Open a GitHub issue with steps to reproduce, the ferrowl version (or commit), and your platform. For TUI rendering issues, the terminal emulator and size are helpful too.
