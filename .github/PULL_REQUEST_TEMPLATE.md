<!--
Ferrowl is spec-driven: docs/specs/ is the authoritative statement of what the
software must do. See CONTRIBUTING.md for the full expectations.

Four sections below, in order. Drop one only when genuinely inapplicable
(e.g. Approach for a one-line fix) — don't drop Verification or its
coverage line.

No tool attribution trailers anywhere in this PR (title, body, or its
commits) — no `Co-Authored-By`, no "Generated with" line.
-->

## Why

<!-- The problem this solves, not a restatement of the diff. -->

## What changed

<!--
New or changed requirement IDs (MB-R-*, OC-R-*, UI-R-*, ...), each quoted with its
normative text so a reviewer does not have to go look it up. Mark changes old -> new.
Write "None — no behavior change." for a refactor, docs or tooling PR.
-->

- `XX-R-000` — "The system shall …"

## Approach

<!-- How the issue was resolved: structure or design decisions the issue itself omitted. -->

## Verification

<!--
What you actually ran, not what could have been run. Be specific: the tests that pin
the new requirements, driving the demo TUI (`cargo run --release -- --demo`), a real
CSMS, a physical device. Paste the outcome if it is interesting.
End with the current line-coverage percentage (`cargo llvm-cov`).
-->

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace -- -D warnings`
- [ ] `cargo check`
- [ ] `cargo test --workspace`
- [ ] Spec updated in `docs/specs/<area>/` — a behavior change with no spec change is incomplete
- [ ] Each new or changed requirement has a test whose doc comment cites its ID
- [ ] README updated, if user-facing commands, keybindings, config fields or the Lua API changed
- Coverage: __% lines (`cargo llvm-cov`)

Closes #
