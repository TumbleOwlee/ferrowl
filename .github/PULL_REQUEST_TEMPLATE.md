<!--
Ferrowl is spec-driven: docs/specs/ is the authoritative statement of what the
software must do. See CONTRIBUTING.md for the full expectations.

Five sections below, in order; one fact per line, lists over sentences. Drop
Tests or Decisions only when genuinely empty — never Verification.

No tool attribution trailers anywhere in this PR (title, body, or its
commits) — no `Co-Authored-By`, no "Generated with" line.
-->

## Why

<!-- What the user gets, one line. What it costs today, one line. -->

## Changes

<!--
One line per crate module: what changed and the requirement IDs it serves.
Spec and README lines last. Write "None — no behavior change." for a refactor,
docs or tooling PR.
-->

- `<crate> <module>` — <what changed> (`XX-R-000`)
- `docs/specs/<area>` — <n> entries added, <n> changed

## Tests

<!--
"<n> of <m> entries pinned", then exceptions only: an entry pinned by a
real-view or integration test instead of a unit test, or not unit-testable
per docs/specs/README.md.
-->

## Verification beyond the gauntlet

<!--
The gauntlet (fmt, clippy, check, test, llvm-cov >= 80 %) is CI-gated and not
listed. Here: what else you actually ran — the demo TUI, a real CSMS, a
physical device — and what you saw. "not run:" is always present.
-->

- <manual run, what was done and seen>
- not run: none
- Coverage: __% lines (`cargo llvm-cov`)

## Decisions

<!-- Design choices the issue did not settle, each with the rejected alternative. -->

Closes #
