# AGENTS.md

Router for AI coding agents working in this repo. Read this first; it points to
everything else.

## Repo

Ferrowl — a Rust TUI simulator for Modbus (client/server, TCP/RTU) and OCPP
(Charging Station/CSMS, versions 1.6/2.0.1/2.1) devices. A Cargo workspace of 13
crates building one `ferrowl` binary. Product: [`PRD.md`](./PRD.md). Structure:
[`ARCHITECTURE.md`](./ARCHITECTURE.md).

<!-- CORE:BEGIN spec-driven -->
## Spec-driven

- `docs/specs/` authoritative. Code conforms to spec, never reverse.
- Read area's `requirements.md` + `edge-cases.md` before editing that area.
  `edge-cases.md` = deliberate ugliness; check before "fixing."
- Behavior change with no spec change = incomplete.
- `main` never holds unfinished spec: a requirement on `main` describes code
  that exists and is tested. A branch may hold a spec commit ahead of its
  code; squash merge keeps that off `main`.
- Pre-existing spec/code disagreement outside your task: stop, raise
  separately. Folding it in widens approved work, skips its own review.
- Specs carry no `file:line`. Locate code with search tools.
- Requirement IDs stable, append-only. Cite in commits and PRs.
<!-- CORE:END spec-driven -->

## TDD — fixed order, every stage

1. Write the test. Doc comment cites requirement ID (`/// MB-R-012 — …`).
2. Run it, watch it fail for the right reason, report the failure. Wrong
   assertion / test-side compile error / premature pass proves nothing.
3. Minimum implementation that passes.
4. Refactor green.

- Implementation without a preceding failing test: not done. Test written
  after the fact to fit code: not done.
- Expected values from the authoritative source (standard/protocol/upstream
  API) — never a debug print of your own implementation.
- Coverage floor 80% of lines, CI-gated. A floor, not a target — never
  inflate it with tests that execute code without asserting.

## Workflow

Triggers on **behavior change, any size**: new public function, changed
default, new error variant, any observable semantics. Not a behavior change:
refactor, rename, perf-with-identical-semantics, tests, docs — no gates, just
do it. Size sets stage count, never gate existence.

- Replaces any generic workflow skill (`/workflow`) — don't run one.
  `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`,
  `fix`, `docs`}.
- **Gate 1 = orchestrator's own conversation with the user, not an agent's.**
  Abstract: existing spec + current goal, nothing about current code. No
  worktree/branch until gate 2 approved.
- Gate 2 onward delegates to agents. **All agents Sonnet or better** —
  weaker models stop mid-plan, commit stubs as "green," report hanging tests
  as verified.
- **Issue and PR belong to the orchestrator alone.** Neither planning nor
  implementing agent is ever told an issue number exists; orchestrator
  updates the issue itself if planning surfaces a spec change.
- **One git worktree per issue per agent**, `.claude/worktrees/<slug>` —
  inside project dir (agent-reachable), gitignored. Two agents in one
  checkout interleave commits — a branch is not a working tree. Created only
  once gate 2 is approved (first thing to touch disk); removed after merge.
- Plan is a contract. Wrong plan → implementer stops and reports, never
  improvises a different design.
- **Sequential gate-2 choice → planning agent continues as implementer**,
  same running agent resumed (not respawned) — exploration behind the plan
  never re-derived. Parallel → fresh implementer per stage (concurrent,
  separate worktrees, can't share a running context).
- **An agent's report of its own verification is not verification.** Re-run
  the tools.
- **Every state change moves a task-board card.** State living only in
  conversation is lost when the session is.
- **An area whose `requirements.md`/`edge-cases.md` has grown large enough
  that reading it costs real context: propose splitting it**, at gate 1,
  before drafting. Split along a real sub-capability boundary already
  present in the area (e.g. `client` → `client-transport` +
  `client-retry`), never an arbitrary line-count cut — a split that isn't
  along a genuine seam just adds a second file covering the same thing.
  New prefix for the new sub-area; **moved requirements keep their original
  ID unchanged** (old prefix and number, just relocated to the new file) —
  IDs are cited in tests, re-IDing them breaks every citation for no reason.
  Only requirements added after the split take the new prefix. Routing
  table updated, same as adding any area. User approves; this doesn't fire
  silently.

Verify before every approval request:
- **Plan** — quoted requirement text matches file, appended IDs genuinely
  unused, nothing contradicts an existing requirement, proposes what was
  asked.
- **Implementation** — re-run full build/test/lint/coverage gauntlet
  yourself in the agent's worktree, read the code described, check ID
  citations sit beside test declarations, mutation-check any
  after-the-fact-looking test, diff spec against approved. Keep only the
  relevant excerpt of any command output in context — failure text, summary
  line — never a full verbose log.

### Task board

State on disk so an interrupted session resumes, not restarts. Directory a
card sits in **is** its state:

```
.claude/tasks/
  open/  inprogress/  inreview/  done/   cards move between these
  artifacts/<slug>/
    spec-diff.md   gate 1 approved normative text
    plan.md        gate 2 stages, steps, dependency tree
    review.md      review findings, keyed by stage id
```

Directories tracked; cards gitignored local state. Cards live in **main
checkout only** — a worktree agent gets its own card's absolute path, writes
only that file. Never a per-worktree board copy.

| Card | File | Owner |
|---|---|---|
| parent | `<slug>.md` | orchestrator — one per run |
| stage | `<slug>.s<n>.md` | the implementer working that stage |
| wave gate | `<slug>.w<n>.md` | orchestrator — one per parallel wave |

Cards are agent-only artifacts, never written for human reading: YAML
frontmatter + append-only log, terse field=value tokens, no prose. Agents
**append**, never rewrite a log line — crash mid-write costs one truncated
line, not the file.

```
---
id: <slug>.s3
parent: <slug>
blocked-by: [<slug>.s2]
files: [src/x.rs, tests/x.rs]
branch: <type>/<slug>-3
worktree: .claude/worktrees/<slug>-3
---
2026-01-02T14:02 spawn agent=impl
2026-01-02T14:05 test-red <ID> rejects_short_input
2026-01-02T14:11 green commit=abc123f
2026-01-02T14:12 gauntlet=pass
```

Parent frontmatter: `issue`, `branch`, `mode: sequential|parallel(N)`,
`gate1`/`gate2` approval dates, current `wave`, `artifacts`. Never the goal or
normative text — issue holds the goal, `artifacts/` holds the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | agent claims green | merged + orchestrator-verified |
| wave gate | not started | stages running | all done; reviewing wave diff | clean — next wave unblocks |
| parent | gate 1 pending | implementing | gate 3/4 | PR squash-merged |

Rules:
- **No agent writes its own `done`.** Implementer stops at `inreview`.
  Orchestrator merges, re-runs gauntlet, only then moves to `done` — same
  self-report rule as everywhere else.
- **Runnable** = every `blocked-by` id in `done/`. Stage `done` = merged into
  the feature branch ("the code I depend on is on the branch I branch from").
- No `blocked/` directory — blocking derives from `blocked-by`, stated once.
- Card is evidence of intent, never fact. Git is fact. See *Resume*.

### Gate 1 — spec diff. Orchestrator runs this itself. Stop for approval.

Not delegated — direct interactive conversation, orchestrator + user, about
existing spec + current goal.

- **No implementation detail.** No code reading, no code-vs-spec check here
  — the dialog outcome decides that. Spec-already-correct → dialog ends with
  no diff: state the violated requirement, continue to gate 2.
- Surface every silent decision (scope, defaults, naming, in/out) one at a
  time, with a recommendation. Reading area `requirements.md`/`edge-cases.md`
  is spec-reading, expected.
- Propose the normative text itself: "shall" statements + appended IDs, plus
  `edge-cases.md` entries. Ready to land, not prose about intent.
- Observable design is spec: public signatures, error enum, feature gating,
  config keys.
- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before the dialog
  — no worktree yet, nothing to put in one. On approval: write
  `artifacts/<slug>/spec-diff.md`, record `gate1` on parent card.

### Gate 1b — tracking issue. Orchestrator runs this itself. Stop for approval.

Search existing issues (`gh issue list`, open and closed) for the same goal;
reuse, never duplicate.

Draft, on GitHub via `gh issue create`, self-contained: a reader with only
the issue can't look a requirement ID up, so quote the full normative text of
every new requirement beside its ID, and every changed one the same way
(old → new), plus the `api-contract.md`/`edge-cases.md` entries. Goal and
normative changes only — never implementation detail (file/function/approach
— that belongs to the plan and the PR). Structure with `##` sections: a
`## Background`/`## Why`, a `## Scope` (or the requirement changes), a
`## Goal`. Keep enumerations compact (grouped ID ranges). Human-friendly
title — plain-language summary a maintainer can scan, not a slug or ID.

Neither planning nor implementing agent is ever told this issue exists.
Orchestrator is sole owner, including later updates from gate 2 findings.

### Gate 2 — implementation plan. Stop for approval.

Spawn the planning agent with a brief: approved spec text, affected area(s),
anything user volunteered at gate 1. Nothing else — gate 1 did no code
research; agent explores the repo itself. Never mention the issue.

Returns: stages of numbered file-level steps; ID→test table; **Verification**
section naming the method (unit tests alone / driving the demo TUI / a real
CSMS); expected commits; expected coverage impact. Existing-code references
are terse and inline at the step needing them — `(file:line)`, never a
paragraph or separate section — minimum words, zero information loss, since
whichever agent implements a stage may lack the planner's own exploration
context. A reference needed by 2+ stages is stated once in the dependency
tree, not repeated at every step that uses it.

May pause with one concise plan-scoped question — answer, it continues. If it
reports a **spec gap** instead (approved text doesn't cover something the
plan needs): stays running, paused; orchestrator reopens gate 1 with the
user, scoped to the gap, then resumes the *same* agent (never respawns) with
settled text, updates issue if it changed.

Plus **Dependency tree**: per stage, dependencies + files touched. No path
between + disjoint files → parallel-capable; else ordered. Read as waves — a
stage is runnable once its dependencies merge. Overlapping files = a
dependency, not a race to resolve later. Fully-sequential plan states so
explicitly — normal outcome, not a decomposition failure.

Approval also settles **how it's implemented** — unanswered = not approved:
- **Sequential** — same planning agent continues as implementer, resumed not
  respawned, keeps its exploration context.
- **Parallel** — user gives max concurrent agents; fresh implementer per
  stage. Waves capped at that number.

Default sequential on no preference. Never infer concurrency from plan shape
— five parallel-capable stages doesn't authorize five agents.

**On approval, in order:**
1. Create the worktree — first thing to touch disk in the run:
   `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
2. Write `artifacts/<slug>/plan.md`, record `gate2`+`mode` on parent card,
   move parent → `inprogress/`.
3. Create `open/<slug>.s<n>.md` per stage, `files`/`blocked-by` copied from
   the tree. Parallel: also `open/<slug>.w<n>.md` per wave. Sequential: one
   wave-gate card for the whole run. Stage ids match plan ids.
4. Land approved spec text in the new worktree — normative only, nothing
   unfinished. First stage, first commit.

### Implement, stage by stage

Sequential: same planning agent, resumed with worktree path + stage cards —
no re-reading `AGENTS.md`, no re-exploring. It reads the implementer rules
itself, follows them per stage in plan order. Moves stage card
`open`→`inprogress` on start, →`inreview` on green+committed. Orchestrator
verifies, moves to `done` — nothing to merge, so `done` = committed+verified.

Parallel: one worktree+branch per agent, branched off the feature branch at
wave start —
`git worktree add .claude/worktrees/<slug>-<n> -b <type>/<slug>-<n> <type>/<slug>`
— **fresh** implementer, not a continuation (concurrent agents can't share a
running context). Plan's inline refs must be self-sufficient because of this — these
agents hold none of the planner's exploration. Never two agents, one
worktree. Each wave:

1. Runnable stage cards (`blocked-by` all in `done/`) up to approved count.
   Wave-gate card → `inprogress/`.
2. One implementer per stage: its worktree path, its stage only, its own
   card's absolute path.
3. Wait for the whole wave; each card lands in `inreview/`.
4. Per card: merge into feature branch, re-run gauntlet on merged result,
   card → `done/`, remove worktree.
5. All `done` → wave gate → `inreview/`: independent reviewer reads the
   wave's accumulated diff. Clean → wave gate `done/`, next wave unblocks, no
   approval prompt. Finding → stop, report, implicated stage cards →
   `inprogress/` with finding appended.

Clean wave gate is not an approval stop — gate 2 already approved these
stages. A finding, red gauntlet, or merge conflict always is.

Merge conflict between two stages in a wave = dependency tree was wrong —
report, fix the tree, never hand-resolve and continue. Mid-wave stop stops
that wave only: finished branches still merge, the rest re-plans.

Gates unchanged under parallelism — verification, review, spec reconcile all
happen once, on the merged feature branch, never per agent.

- TDD order above. Stage = green checkpoint: builds, tests pass, lint clean,
  coverage ≥ 80%. Commit every green stage — makes the plan resumable.
- Stage messages cheap, squashed later. Squash message carries requirement
  IDs + why. Spec = first stage, first commit.
- **Never add `Co-Authored-By`, "Generated with," or any tool attribution
  trailer** to a commit, PR body, issue, or comment — no information, pure
  `git log` noise, forever. Applies to every agent, every gate, including the
  squash message and the gate 4 PR body.
- Every new/changed requirement ships ≥1 ID-citing test.
- Every existing test pinning observable behavior cites its requirement.
  Pure internal/helper-detail tests may stay untagged. Behavior no
  requirement states = requirement missing — add it (gate 1), never attach a
  loose ID.
- Citation directly beside the test declaration, above the function body.
  ≤1 ID per test.
- Not done until the Verification method has run and its outcome is
  reported. Waiving it requires asking.

### Reconcile the spec

Behavior differing from gate 1 approval is normative, **reopens gate 1**:
show the diff, state what forced it, get approval before committing. Wrong
cross-reference / clumsy wording = editorial, no approval needed. Always
report the final spec diff.

### Gate 3 — review. Stop for approval.

Before proposing a PR: independent review in a **separate agent** that didn't
write the code (a reviewer sharing the implementer's context reproduces its
blind spots). Give it the diff, approved spec text, the artifact dir, the
worktree path — never the issue number, same as every agent in this workflow.
It reads its own rules (`.claude/AGENTS.core.md`) itself. Reports:

- **Spec fidelity** — every approved requirement implemented, nothing
  unapproved implemented (scope creep is a finding even if the code is
  good), no ID pinned by a test that doesn't actually exercise it.
- **Standards** — conventions below, test naming, ID citation, error
  handling.
- **TDD honesty** — tests passing against an empty implementation,
  assertions on the implementation's own output, coverage padded by
  non-asserting tests.

Re-run the verification yourself, report findings + fixes. User-decision
findings are raised, not silently fixed.

**Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage
id so the right cards return to `inprogress/`. Parent card → `inreview/`
when review starts.

### Gate 4 — pull request. Stop for approval.

- Verification run + reported, then **ask whether to open a PR** — user may
  want a manual run first; don't pre-empt it.
- Draft title + body, get approval of that text, push, open.
- Title plain language, issue's style. Body = the implementation: why,
  requirement IDs, how the issue was resolved (approach/structure the issue
  omitted), verification actually performed, the coverage number, and
  `Closes #<issue>`.

### Merge

Squash merge to `main` — stage commits, including the ahead-of-code spec
commit, never reach `main`. Then:

```sh
git worktree remove .claude/worktrees/<slug>
git worktree list   # nothing under .claude/worktrees/ should remain
```

Per-wave worktrees are already removed at wave end; this sweep catches
stragglers from a stopped agent. Parent card → `done/` — no card for this
run stays outside `done/`.

### Resume an interrupted run

Cards outside `open/`+`done/`, no agent running = session died mid-run.
Resume triggered by the user or `/spec-feature`, never automatic.

No worktree recorded on the card → died during gate 1 dialog or gate 2
planning, nothing on disk to reconcile — resume the conversation from
`spec-diff.md`/`plan.md`'s last state. Past gate 2 → table below either way.
**Any resumed implementation spawns a fresh agent** — the sequential
continuation only lives inside a live orchestrator session, doesn't survive
a crash. This is why plan refs must be lossless: a fresh implementer
resuming mid-plan gets nothing but what the plan wrote down.

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `gauntlet=pass` | re-run at that sha | card overstated state |
| stage `done` | `git branch --contains` vs feature branch | never merged; downstream plans a lie |

Report differences first. Agree → resume. Disagree → stop and report: card
behind git is a forgotten move, correctable; card claiming what git can't
show is never trusted into being true.

Clean reconcile → resume only no-approval work (respawn implementers for
approved stages, merge finished branches, run wave gates); halt at the first
gate needing the user. Recorded `gate1`/`gate2` approvals stay valid, no
re-ask.

## Where to look for task X

| Task touches | Read | ID prefix |
|---|---|---|
| Modbus register codec, store, client/server (TCP/RTU) | [`docs/specs/modbus/`](./docs/specs/modbus/) | `MB-R-*` |
| OCPP actions, CS/CSMS engine, versions 1.6/2.0.1/2.1, TLS/auth | [`docs/specs/ocpp/`](./docs/specs/ocpp/) | `OC-R-*` |
| Lua scripting (`C_*` API, sim threads, sandbox) | [`docs/specs/scripting/`](./docs/specs/scripting/) | `SC-R-*` |
| TUI widgets, dialogs, `:` commands, keybindings, code editor | [`docs/specs/tui/`](./docs/specs/tui/) | `UI-R-*` |
| Config/session file format, save/load, `migrate` | [`docs/specs/config-session/`](./docs/specs/config-session/) | `CS-R-*` |
| CLI flags, `ferrowl run` headless, exit codes | [`docs/specs/cli-headless/`](./docs/specs/cli-headless/) | `CL-R-*` |
| Platforms, performance, security, versioning, testing conventions | [`docs/specs/non-functional-requirements.md`](./docs/specs/non-functional-requirements.md) | `NF-R-*` |
| Crate graph, data flow, concurrency model | [`ARCHITECTURE.md`](./ARCHITECTURE.md) | — |
| Contribution workflow, conventions | [`CONTRIBUTING.md`](./CONTRIBUTING.md) | — |

Each area's `edge-cases.md` records its **known limitations** — behavior that
is ugly but intentional. Check it before "fixing" something that looks wrong.

<!-- CORE:BEGIN build -->
## Build / test / lint

```sh
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo check --workspace
cargo test --workspace
cargo llvm-cov --workspace --fail-under-lines 80
```

Narrow the loop while iterating — don't run the whole workspace for one test:

```sh
cargo test -p ferrowl-modbus              # one crate
cargo test -p ferrowl-codec ut_decode     # one test (unit tests are named ut_*)
cargo check -p ferrowl-ocpp               # typecheck one crate
cargo llvm-cov --workspace --html         # browsable per-line coverage
```

Full set before done. `lefthook` enforces `fmt --check` and `clippy -D
warnings` pre-commit; CI runs the full set (plus `cargo deny check`) on every
push **and every pull request**.

Dev loop: `cargo run --release -- --demo` (built-in demo tabs, no config
needed) or `cargo build --profile fastrel` for faster iterative builds
(opt-level 1).
<!-- CORE:END build -->

<!-- CORE:BEGIN conventions -->
## Conventions

- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file under
  test, function names prefixed `ut_`. Integration tests belong in each
  crate's `tests/`, function names prefixed `it_` (notably in `ferrowl-ui`
  and much of `ferrowl`).
- **Tests bind ephemeral ports only.** Any test that starts a network
  listener (Modbus TCP, OCPP WebSocket, a raw occupier socket) binds port 0
  and reads the assigned port back — never a fixed port number. A fixed port
  makes the test fail whenever anything else on the machine holds it (a
  running `ferrowl --demo`, a parallel checkout's tests); ephemeral binding
  removes the collision class entirely. Fixed ports in specs that are never
  `start()`ed are inert, but prefer port 0 there too so the intent stays
  obvious. Deliberately *occupying* a port to test bind-failure handling
  still binds the occupier ephemerally first, then points the server at that
  port (see `tcp_loopback.rs`).
- All 13 workspace crates are versioned in lockstep. Don't bump one
  independently.
- Config files are TOML or JSON only (extension-driven), never YAML.
- Rust edition 2024, stable toolchain (`rust-toolchain.toml`).
- **Never split a source file just because it is large.** A split must earn
  its keep — it separates genuinely distinct responsibilities, improves
  navigability, or cuts coupling. A long file that covers one cohesive
  concern, or is flat generated data (e.g. a spec table), stays whole. Treat
  a line count as a prompt to *review* the file, not a mandate to divide it;
  an arbitrary boundary drawn to hit a number makes the code harder to
  follow, not easier.
- **Prefer typed handling over generic JSON.** Read request fields and build
  responses from the strongly-typed `rust_ocpp` structs and enums
  (`req.evse_id`, `Response201::Reset(...)`), never by indexing or
  hand-crafting a `serde_json::Value` where a typed path exists — the
  compiler must catch a wrong field name, missing field, or bad enum, not the
  wire. This holds **even when it forces duplication**: if two OCPP versions
  (1.6 / 2.0.1 / 2.1) have distinct-but-similar types, duplicate the typed
  code per version rather than collapse it onto a shared untyped `Value`.
  The duplication is the accepted price of compile-time typing. The **only**
  sanctioned untyped JSON is the manual payload a user types into the action
  dialog, and version-independent *plumbing* that must inspect an arbitrary
  encoded action (e.g. a scope/EVSE guard spanning all actions) where no
  typed accessor exists.
<!-- CORE:END conventions -->

<!-- CORE:BEGIN scope -->
## Scope boundaries — check with the user before

- **Expanding the Lua `C_*` API.** The surface (`C_Register`, `C_OCPP`,
  `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`) is deliberately small
  and fixed. Adding a module or method is a design decision, not a
  mechanical addition.
- **Bridging Modbus and OCPP.** They are architecturally separate — no
  shared lifecycle abstraction spans both. Don't assume a fix/pattern in one
  applies to the other without checking both specs.
<!-- CORE:END scope -->
