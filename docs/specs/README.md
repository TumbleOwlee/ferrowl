# Ferrowl Specs

Authoritative spec of `ferrowl`'s behavior, by capability area. **Normative**: code conforms to these, not vice versa. Code/spec disagreement = defect in one — resolve, don't paper over.

## Areas

| Area | Covers |
|---|---|
| [`modbus/`](./modbus/) | Modbus client & server, TCP + RTU, register model, formats, codec, reconnect |
| [`ocpp/`](./ocpp/) | OCPP Charging Station & CSMS, versions 1.6/2.0.1/2.1, actions, TLS/auth |
| [`scripting/`](./scripting/) | Lua sim model, `C_*` API contract, execution & error semantics |
| [`tui/`](./tui/) | Tabs, dialogs, `:` commands, keybindings, editor |
| [`config-session/`](./config-session/) | Device/session file envelope, TOML/JSON, save/load, `migrate` |
| [`cli-headless/`](./cli-headless/) | `ferrowl run`, CLI flags, exit codes, CI usage |
| [`bridge/`](./bridge/) | `ferrowl bridge`, headless upstream/downstream Modbus relay (TCP/RTU) |

Cross-cutting: [`non-functional-requirements.md`](./non-functional-requirements.md) (`NF-R-nnn`).

## Rules for writing specs

1. **No code pointers.** Never cite file:line, function/type names, crate-internal identifiers — specs state *what must be true*, not where implemented (code pointers rot on refactor). Exception: public, user-facing surface (config keys, `:` commands, Lua `C_*` API, CLI flags, OCPP action names, exported signatures, error variants, feature flags) IS spec content → area's `api-contract.md`.
2. **Requirement IDs stable, append-only.** ID = area prefix + number: `MB-R-nnn` modbus, `OC-R-nnn` ocpp, `SC-R-nnn` scripting, `UI-R-nnn` tui, `CS-R-nnn` config-session, `CL-R-nnn` cli-headless, `BR-R-nnn` bridge, `NF-R-nnn` non-functional (global). Never renumber, never reuse a retired ID (deleted requirement's ID stays dead). Reference by ID in commits/PRs/tests/agent instructions.
3. **Owner = the behavior, not the surface.** A Modbus RTU config field is specified in `modbus/`, not `config-session/` — it belongs with the behavior it controls (one change → one file). `config-session/` owns only the *envelope*: file format, `version` field, the session→module list, save/load and `migrate` semantics. `tui/` owns the command mechanism and generic commands; protocol-specific commands live in their protocol's area. Shared behavior across areas → the owning area, stated once.
4. **Requirements are testable.** "Shall" statements, observable outcomes. Good: "The client shall retry with exponential backoff bounded to 1s–30s." Bad: "The client is robust." Format-involving requirements: name the exact bytes where possible.
5. **Known gaps are specified, not hidden.** Intentional-but-ugly behavior (no Lua execution ceiling, no OCPP auto-reconnect) → area's `edge-cases.md` as a stated constraint, so it isn't mistaken for an oversight and "fixed".
6. **One requirement, one physical line.** No line break inside a `**<PREFIX>-R-nnn** — ...` statement, however long — `grep -rn <ID> docs/specs/` (or any keyword from the text) must return the complete requirement in a single match, not a truncated first line. Find one or more by ID, with the exact file:line to edit: `sh .claude/scripts/extract-id.sh <ID> [<ID> ...]` (searches `docs/specs` by default; batch every ID needed into one call). To read a whole section instead of a whole file, use `sh .claude/scripts/extract-section.sh '## <heading>' docs/specs/<area>/requirements.md`.

## Per-area files

Not every area needs every file — add/drop per need.

| File | Contains |
|---|---|
| `requirements.md` | Numbered, testable "shall" statements. Every area has one. |
| `api-contract.md` | Stable public surface: OCPP actions, Lua `C_*` methods, `:` commands, keybindings, CLI flags, config fields, error variants, feature flags. |
| `data-contract.md` | Formats: register model and data formats, payload shapes, config schema, field widths, ordering, ranges. |
| `edge-cases.md` | Boundary behavior, error semantics, stated known limitations. |

## Requirements intentionally not unit-tested

Most requirements are pinned by a test whose doc comment cites the ID. A minority are **deliberately** untested — not gaps; this list records that decision. Only three kinds qualify:

1. **Design-posture/platform/toolchain/versioning statements** — assert facts about build/design, not runtime behavior a `shall` test could observe. Each names its enforcement point (CI job, manifest field, lint config) instead.
2. **Cross-cutting restatements whose behavior is asserted under the owning area** — requirement is real, but its test lives with the per-area requirement that owns the behavior, cited by *that* ID.
3. **Structural/shape requirements collectively exercised by one round-trip or property test, not per-requirement** — a group of requirements describing the shape of a data structure (which fields exist, which are optional, default omission), covered together by a save→load→compare round-trip; the behavioral requirements around them are tested on their own.

Anything not listed below must carry a citing test. If a requirement here later gains observable behavior worth pinning directly, remove it from the list and add the test.

**Kind 1 — design posture/platform/toolchain/versioning**

| Requirement | Enforced by |
|---|---|
| `NF-R-001`, `NF-R-002`, `NF-R-003` — which platforms/toolchain CI builds | CI job matrix |
| `NF-R-010` — no benchmarks asserted; hot path stays on `parking_lot` | design posture, dependency manifest |
| `NF-R-040` — crates versioned in lockstep | workspace manifest |
| `NF-R-041` — the testing conventions themselves | `AGENTS.md` conventions, lefthook reminder |
| `UI-R-001` — alt-screen + raw-mode entry, terminal restore on normal/error/panic exit | terminal-platform fact: the restore path calls the real terminal (`enable_raw_mode`/`disable_raw_mode`), which errors or panics under `cargo test` without a controlling tty — the reason `App` renders through the `DrawSurface` seam. The seam is exercised headlessly (`ut_app_draws_onto_mock_screen`); the raw-mode/panic-hook control itself is not observable by a `shall` test |

**Kind 2 — cross-cutting restatements**

| Requirement | Asserted under |
|---|---|
| `NF-R-011` — Lua sim on its own OS thread | `scripting/` sim tests |
| `NF-R-020` — Modbus reconnect backoff | `MB-R-050`/`MB-R-051`/`MB-R-052` |
| `NF-R-021` — OCPP no auto-reconnect | `OC-R-048` |
| `NF-R-022` — a Lua error never crashes its host | `SC-R-032` |
| `NF-R-030` — OCPP TLS / Basic Auth | OCPP security tests (`OC-R-029`–`041`) |
| `NF-R-031` — Lua sandbox | `SC-R-006`/`SC-R-007` |

**Kind 3 — structural/shape (collectively exercised)**

| Requirements | Exercised by |
|---|---|
| `CS-R-001`, `CS-R-010`, `CS-R-015`, `CS-R-016`, `CS-R-018`, `CS-R-020`, `CS-R-021`, `CS-R-022`, `CS-R-034` — shape of a valid session/device file (which fields exist, which are informational, default omission) | config round-trip (serde) tests: save→load→compare. Envelope behavior (save/load, `migrate`) is tested on its own |

## Keeping specs true

Before changing code in an area, read that area's `requirements.md`. Change contradicts spec → update spec **in the same commit** (behavior change with no spec change = incomplete change). Full gated workflow: [`AGENTS.md`](../../AGENTS.md).
