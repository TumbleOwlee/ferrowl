# AGENTS.md

Router for AI coding agents working in this repo. Read this first; it points to
everything else.

## What this repo is

Ferrowl — a Rust TUI simulator for Modbus (client/server, TCP/RTU) and OCPP
(Charging Station/CSMS, versions 1.6/2.0.1/2.1) devices. A Cargo workspace of 12
crates building one `ferrowl` binary. Product framing: [`PRD.md`](./PRD.md).
Structure and crate map: [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Spec-driven — read this before you change behavior

`docs/specs/` is the **authoritative** specification: the code is expected to
conform to it, not the other way around. Before you edit code in an area, read that
area's `requirements.md`. A behavior change with no spec change is incomplete — the
workflow below is how the two stay together.

**`main` never contains an unfinished spec.** A requirement on `main` is a statement
about code that exists and is tested. A feature branch may hold a spec commit ahead of
its implementation (see the workflow); `main` may not, which the squash merge
guarantees.

If the code and the spec already disagree and it is *not* what you were asked to fix:
**stop and raise it as its own task.** Do not fold the fix into the change in flight —
it silently widens work that was already approved, and the fix deserves its own review.

Specs contain no `file:line` pointers by design — locate code with your own search
tools. Requirements have stable IDs (`MB-R-*`, `OC-R-*`, …); reference them in
commits and PRs.

## Workflow — follow this for every behavior change

This project's workflow **replaces** any generic workflow skill (including `/workflow`);
do not run one here. `docs/specs/` already serves as the PRD and the design record — a
second design-artifact system would only give the "why" two homes to diverge in.

**It triggers on behavior change, not on size.** Ask: *does this change what the
software is required to do?* If yes — a new feature, a changed keybinding, different
observable semantics — the full workflow applies, however small the diff. If no — a
refactor, a rename, perf work with identical semantics, tests, docs — there is no spec
diff to approve, so skip the gates and just do the work. Size decides how many *stages*
the plan has, never whether the gates exist.

Work on a branch off `main`, never on `main` itself. `<type>/<slug>`, conventional-commit
type (`feat/`, `fix/`, `docs/`).

1. **Read the affected area's spec.** Use the routing table below to find it. Read
   `requirements.md` and `edge-cases.md` before proposing anything — `edge-cases.md`
   records behavior that is ugly *on purpose*.
2. **Gate 1 — the behavior contract.** Propose the **spec diff itself**: the actual
   "shall" text of the new or changed requirements, with their appended IDs, plus any
   `edge-cases.md` entries. Not prose about what you intend to build — the normative
   text, ready to land. Design choices that are observable *are* spec, and get settled
   here. **Stop for approval.** For a bug fix where the spec is already right and the
   code is wrong, there is no diff to approve: state the requirement the code violates
   and move on.
3. **Gate 1b — the tracking issue.** Once the spec is approved, search the repo's open
   issues (`gh issue list`, plus a search of closed ones) for anything with the **same
   goal**. If one exists, use it — reference its number from here on, do not open a second.
   If none exists, draft the issue body and **stop for approval**; create it with
   `gh issue create` only once confirmed.

   The issue must be **self-contained**: at this point the spec lives only in the working
   tree, so a reader who has only the issue cannot look a requirement ID up. **Always quote
   the full normative text** of every new requirement next to its ID, and list every
   *changed* requirement the same way (old → new), plus the `api-contract.md` and
   `edge-cases.md` entries. An ID with no text is useless to the reader.

   Keep the issue free of **implementation detail** — it states the goal and the normative
   changes, not how the code will be structured. That belongs to gate 2.
4. **Write the spec into the working tree.** Do not mark it "unfinished" in the file —
   the file only ever contains normative text. The plan tracks what is not yet backed by
   a passing test.
5. **Gate 2 — the implementation plan.** Stages, file-level steps, a table mapping each
   new requirement ID to the test that will pin it, and a **Verification** section naming
   how the change will be exercised (tests alone; driving the demo TUI; a real CSMS).
   State the expected commits. **Stop for approval.**
6. **Implement, stage by stage.** A stage is a **green checkpoint**: it compiles,
   `cargo test --workspace` passes, `clippy -D warnings` passes. **Commit every green
   stage** — that is what makes the plan resumable after an interrupted session. Stage
   commits are branch-local scaffolding and are squashed away on merge, so keep their
   messages cheap; the squash message is the one that must carry the requirement IDs and
   the why. The spec is the first stage, hence the first commit — legal on a branch,
   never on `main`.
   Every new or changed requirement ships with at least one test whose doc comment cites
   its ID (`/// UI-R-051 — …`). Do not backfill IDs onto existing tests.
   The task is not done until the plan's Verification method has actually been run and
   its outcome reported. Waiving it requires asking.
7. **Reconcile the spec.** If implementation forced the behavior to differ from what
   gate 1 approved, the "shall" text changes — that is a **normative** change and it
   **re-opens gate 1**: show the diff, say what forced it, get approval before
   committing. Fixing a wrong cross-reference or clumsy wording is **editorial** and
   needs no approval. **Always report the final spec diff** when you finish, so the
   difference between the two is visible without diffing by hand.
8. **Gate 3 — the pull request.** With the work done, the Verification method run and its
   outcome reported: **stop and ask whether to open a PR.** The user may want a manual
   test run of their own first — that is the point of this gate, so do not pre-empt it.
   Once they confirm, draft the PR title and body (the why, the requirement IDs, the
   verification actually performed, `Closes #<issue>` from gate 1b) and **stop for
   approval** of that text. Only then push the branch and `gh pr create`.

Merge to `main` by **squash merge**, so the branch's stage commits — including the spec
commit that briefly ran ahead of its code — never reach `main`.

## Where to look for task X

| Task touches | Read |
|---|---|
| Modbus register codec, store, client/server (TCP/RTU) | [`docs/specs/modbus/`](./docs/specs/modbus/) |
| OCPP actions, CS/CSMS engine, versions 1.6/2.0.1/2.1, TLS/auth | [`docs/specs/ocpp/`](./docs/specs/ocpp/) |
| Lua scripting (`C_*` API, sim threads, sandbox) | [`docs/specs/scripting/`](./docs/specs/scripting/) |
| TUI widgets, dialogs, `:` commands, keybindings, code editor | [`docs/specs/tui/`](./docs/specs/tui/) |
| Config/session file format, save/load, `migrate` | [`docs/specs/config-session/`](./docs/specs/config-session/) |
| CLI flags, `ferrowl run` headless, exit codes | [`docs/specs/cli-headless/`](./docs/specs/cli-headless/) |
| Platforms, performance, security, versioning | [`docs/specs/non-functional-requirements.md`](./docs/specs/non-functional-requirements.md) |
| Crate graph, data flow, concurrency model | [`ARCHITECTURE.md`](./ARCHITECTURE.md) |
| Contribution workflow, conventions | [`CONTRIBUTING.md`](./CONTRIBUTING.md) |

Each area's `edge-cases.md` records its **known limitations** — behavior that is
ugly but intentional. Check it before "fixing" something that looks wrong.

## Build / test / lint

```sh
cargo check
cargo test --workspace
cargo clippy --workspace       # CI and lefthook run with -D warnings
cargo fmt --check
```

Narrow the loop while iterating — don't run the whole workspace for one test:

```sh
cargo test -p ferrowl-modbus              # one crate
cargo test -p ferrowl-codec ut_decode     # one test (unit tests are named ut_*)
cargo check -p ferrowl-ocpp               # typecheck one crate
```

Run these before considering work done — `lefthook` enforces `fmt --check` and
`clippy -D warnings` pre-commit, and CI runs all four as separate steps of the `check`
pipeline on every push **and every pull request**, so a lint failure is caught either way.

Dev loop: `cargo run --release -- --demo` (built-in demo tabs, no config needed) or
`cargo build --profile fastrel` for faster iterative builds (opt-level 1).

## Conventions

- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file under test,
  function names prefixed `ut_`. For integration tests, the function names are 
  prefixed with `it_`  (notably in `ferrowl-ui` and much of `ferrowl`). Integration
  tests belong in each crate's `tests/`.
- All 12 workspace crates are versioned in lockstep. Don't bump one independently.
- Config files are TOML or JSON only (extension-driven), never YAML.
- Rust edition 2024, stable toolchain (`rust-toolchain.toml`).

## Scope boundaries — check with the user before

- **Expanding the Lua `C_*` API.** The surface (`C_Register`, `C_OCPP`, `C_Time`,
  `C_Log`, `C_Test`, `C_Module`, `C_Statics`) is deliberately small and fixed.
  Adding a module or method is a design decision, not a mechanical addition.
- **Bridging Modbus and OCPP.** They are architecturally separate — no shared
  lifecycle abstraction spans both. Don't assume a fix/pattern in one applies to
  the other without checking both specs.
