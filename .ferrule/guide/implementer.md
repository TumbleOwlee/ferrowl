## Project rules

`AGENTS.md` is already in your context (`CLAUDE.md` includes it); never re-read it. Binding here: `## Spec-driven`, `## TDD — fixed order, every stage`, `## Workflow` (the `files` contract), `## Build / test / lint`, `## Conventions — reading`, `## Conventions — code`, `## Conventions — text`, `## Scope boundaries — check with the user before`. Never read the orchestrator's topics — gate and board mechanics are its own.

## Checks

- `cargo fmt --check`
- `cargo clippy --workspace -- -D warnings` — type-checks and lints every crate, so a dependent crate your change broke fails here
- `cargo test -p <crate>` for every crate whose files you changed
