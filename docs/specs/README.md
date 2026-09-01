# Ferrowl Specs

Authoritative spec of `ferrowl` behavior, by capability area. **Normative**: code conforms to these, never vice versa. Code/spec disagreement = defect in one; resolve, don't paper over.

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

1. **No code pointers.** Never cite file:line, function/type names, crate-internal identifiers — specs state *what must be true*, not where (pointers rot on refactor). Exception: public, user-facing surface (config keys, `:` commands, Lua `C_*` API, CLI flags, OCPP action names, exported signatures, error variants, feature flags) IS spec → area's `api-contract.md`.
2. **Requirement and edge-case IDs stable, append-only.** ID = area prefix + number: `MB-R-nnn` modbus, `OC-R-nnn` ocpp, `SC-R-nnn` scripting, `UI-R-nnn` tui, `CS-R-nnn` config-session, `CL-R-nnn` cli-headless, `BR-R-nnn` bridge, `NF-R-nnn` non-functional. Never renumber, never reuse a retired ID. Reference by ID in commits/PRs/tests/agent instructions. Edge-case IDs are a second, independent series over `edge-cases.md` entries: `MB-E-nnn`, `OC-E-nnn`, `SC-E-nnn`, `UI-E-nnn`, `CS-E-nnn`, `CL-E-nnn`, `BR-E-nnn`, numbered independently per area, same stability rules (never renumber, never reuse a retired ID); no `NF-E` series (`non-functional-requirements.md` has no `edge-cases.md`). An edge-case entry derived from a requirement cites that `-R` ID in its own text and still carries its own `-E` ID: the `-E` ID identifies the entry, the `-R` ID names the requirement it follows from. Tests may cite an `-E` ID exactly as they cite an `-R` ID; the "At most one ID per test" rule below still applies.
3. **Owner = the behavior, not the surface.** A Modbus RTU config field is specified in `modbus/`, not `config-session/` (one change → one file). `config-session/` owns only the *envelope*: file format, `version`, session→module list, save/load, `migrate`. `tui/` owns the command mechanism and generic commands; protocol-specific commands live in their protocol's area. Shared behavior → owning area, stated once.
4. **Requirements are testable.** "Shall" statements, observable outcomes. Good: "The client shall retry with exponential backoff bounded to 1s–30s." Bad: "The client is robust." Format requirements name exact bytes where possible.
5. **Known gaps are specified, not hidden.** Intentional-but-ugly behavior (no Lua execution ceiling, no OCPP auto-reconnect) → area's `edge-cases.md` as a stated constraint, so it isn't "fixed".
6. **One requirement or edge-case entry, one physical line.** No line break inside a `**<PREFIX>-R-nnn** — ...` or `**<PREFIX>-E-nnn** — ...` statement, however long, table rows included — `grep -rn <ID> docs/specs/` (or any keyword) must return the whole entry in one match. Exact file:line by ID: `sh .claude/scripts/extract-id.sh <ID> [<ID> ...]` (batch IDs into one call). One section: `sh .claude/scripts/extract-section.sh '## <heading>' docs/specs/<area>/requirements.md`.
7. **Contract-file citations.** `api-contract.md`/`data-contract.md` carry no ID series of their own; every table row carries a `Req` column naming the owning requirement ID(s), prose entries cite inline. IDs are always enumerated, never given as a range. A row with no owning requirement is marked `—` and raised as a missing requirement, never given an invented ID. Cross-references between spec files use an `-R`/`-E` ID when the target has one, and the target's exact heading text (the string `extract-section.sh` accepts) otherwise — section numbers (`§n.m`) are not used.
8. **At most one ID per test.** A test's doc comment cites exactly one requirement or edge-case ID, the one it most directly pins; a test touching several requirements cites the primary one and lets the others stay implicit or get their own dedicated test.

## Per-area files

Add/drop per need.

| File | Contains |
|---|---|
| `requirements.md` | Numbered, testable "shall" statements. Every area has one. |
| `api-contract.md` | Stable public surface: OCPP actions, Lua `C_*` methods, `:` commands, keybindings, CLI flags, config fields, error variants, feature flags. No ID series of its own; each table row's `Req` column names the owning requirement ID(s). |
| `data-contract.md` | Formats: register model and data formats, payload shapes, config schema, field widths, ordering, ranges. No ID series of its own; each table row's `Req` column names the owning requirement ID(s). |
| `edge-cases.md` | Boundary behavior, error semantics, stated known limitations. Entries carry their own `-E-nnn` IDs. |

## Requirements intentionally not unit-tested

Most requirements are pinned by a test citing the ID. This list records the deliberate exceptions. Four kinds qualify:

1. **Design-posture/platform/toolchain/versioning statements** — facts about build/design, not runtime behavior a `shall` test observes. Each names its enforcement point (CI job, manifest field, lint config).
2. **Cross-cutting restatements asserted under the owning area** — the test lives with the per-area requirement, cited by *that* ID.
3. **Structural/shape requirements collectively exercised by one round-trip or property test** — a group describing a data structure's shape (fields, optionality, default omission), covered by one save→load→compare; behavioral requirements around them tested on their own.
4. **Stated known limitations (`-E` entries)** — an `-E` entry that records a known limitation or an observed constraint rather than an asserted behavior is exempt as a class, with no enumeration of IDs. An `-E` entry that does assert behavior (most boundary-table rows) is outside the exemption and wants a citing test. This obligation binds `-E` entries added after the backfill that introduced the `-E` series; every entry minted by that backfill is grandfathered, and an asserting entry acquires its citing test when its area is next touched for behavior.

Anything not listed below, and no `-E` entry outside kind 4, must carry a citing test. A listed requirement that gains observable behavior: remove from the list, add the test.

**Kind 1 — design posture/platform/toolchain/versioning**

| Requirement | Enforced by |
|---|---|
| `NF-R-001`, `NF-R-002`, `NF-R-003` — platforms/toolchain CI builds | CI job matrix |
| `NF-R-010` — no benchmarks asserted; hot path stays on `parking_lot` | design posture, dependency manifest |
| `NF-R-040` — crates versioned in lockstep | workspace manifest |
| `NF-R-041` — the testing conventions themselves | `AGENTS.md` conventions, lefthook reminder |
| `NF-R-045` — dev-only fixture crate, `publish = false`, versioned in lockstep | workspace manifest, dev-dependency edges |
| `UI-R-001` — alt-screen + raw-mode entry, terminal restore on normal/error/panic exit | terminal-platform fact: the restore path calls the real terminal (`enable_raw_mode`/`disable_raw_mode`), which errors or panics under `cargo test` without a controlling tty — why `App` renders through the `DrawSurface` seam. Seam exercised headlessly (`ut_app_draws_onto_mock_screen`); raw-mode/panic-hook control itself not observable by a `shall` test |

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
| `CS-R-001`, `CS-R-010`, `CS-R-015`, `CS-R-016`, `CS-R-018`, `CS-R-020`, `CS-R-021`, `CS-R-022`, `CS-R-034` — shape of a valid session/device file (fields, informational fields, default omission) | config round-trip (serde) tests: save→load→compare. Envelope behavior (save/load, `migrate`) tested on its own |

**Kind 4 — stated known limitations (`-E` entries)**

No ID table — this kind is a class-wide exemption, not an enumerated list. An `-E` entry that records a known limitation or an observed constraint rather than an asserted behavior is exempt as a class. An `-E` entry that does assert behavior (most boundary-table rows) is outside the exemption and wants a citing test. This obligation binds `-E` entries added after the backfill that introduced the `-E` series; every entry minted by that backfill is grandfathered, and an asserting entry acquires its citing test when its area is next touched for behavior.

## Keeping specs true

Before changing code in an area, read its `requirements.md`. Change contradicts spec → update spec **in the same commit** (behavior change with no spec change = incomplete). Full gated workflow: [`AGENTS.md`](../../AGENTS.md).
