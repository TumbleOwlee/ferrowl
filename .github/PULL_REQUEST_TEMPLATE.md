<!--
Ferrowl is spec-driven: docs/specs/ is the authoritative statement of what the
software must do. See CONTRIBUTING.md for the full expectations.
Delete any section that genuinely does not apply.
-->

## Why

<!-- The problem this solves, not a restatement of the diff. -->

## Requirements

<!--
New or changed requirement IDs (MB-R-*, OC-R-*, UI-R-*, ...), each quoted with its
normative text so a reviewer does not have to go look it up. Mark changes old -> new.
Write "None — no behavior change." for a refactor, docs or tooling PR.
-->

- `XX-R-000` — "The system shall …"

## Verification

<!--
What you actually ran, not what could have been run. Be specific: the tests that pin
the new requirements, driving the demo TUI (`cargo run --release -- --demo`), a real
CSMS, a physical device. Paste the outcome if it is interesting.
-->

## Checklist

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace -- -D warnings`
- [ ] `cargo check`
- [ ] `cargo test --workspace`
- [ ] Spec updated in `docs/specs/<area>/` — a behavior change with no spec change is incomplete
- [ ] Each new or changed requirement has a test whose doc comment cites its ID
- [ ] README updated, if user-facing commands, keybindings, config fields or the Lua API changed

Closes #
