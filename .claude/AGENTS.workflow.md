# AGENTS.workflow.md

Gate/task-board workflow mechanics, split out of `AGENTS.md` (see that
file's `## Workflow` pointer) so an eager `AGENTS.md` read doesn't pay
for gate mechanics on sessions that never run the spec-feature workflow.
Orchestrator only — `AGENTS.core.md` readers (spec-author/planner/
implementer/reviewer agents) never read this file.

## Workflow

What triggers a gated run is `AGENTS.md`'s `## Workflow`. Every heading
below is one `extract-section.sh` pull; never read this file whole.

### Principles

- Replaces any generic workflow skill (`/workflow`) — don't run one.
  `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`,
  `fix`, `docs`}. **Enforced, not just advisory:** the same `PreToolUse` hook
  (`.claude/scripts/hook-guard-cat.sh`) denies `git commit` while the
  checkout is on `main`, and `git push` targeting `main` — the safety net
  for an agent that missed the worktree step, not just a written rule.
- **The orchestrator never changes spec or code and never reads either.**
  It spawns agents, relays one question at a time between an agent and the
  user, runs git/`gh` plumbing (worktree add/remove, merge, push, `gh …
  --body-file`), runs `.claude/scripts/gauntlet.sh`, and moves cards. Every
  other output — spec text, issue body, plan, code, review, PR body — is a
  file an agent wrote under `artifacts/<slug>/` or in a worktree. The
  orchestrator hands paths around, never file contents; a user who wants
  detail is pointed at the path. See *Agent hand-off*.
- **Gate 1 is a dialog with the user, drafted by `spec-author`.** Abstract:
  existing spec + current goal, nothing about current code. No
  worktree/branch until gate 2 approved.
- **No spec effect (docs-only, other non-functional change with no
  observable-behavior change) → skip gate 1 entirely**, proceed straight to
  gate 2. No `spec-diff.md`, no `gate1` on the parent card. If gate 2
  planning turns up an actual spec gap, stop and run gate 1 before
  continuing — same as any other spec-gap pause.
- Gate 2 onward delegates to agents. **All agents Sonnet or better** —
  weaker models stop mid-plan, commit stubs as "green," report hanging tests
  as verified.
- **Issue and PR are filed by the orchestrator alone**, from a body file
  `spec-author` drafted. Neither planning nor implementing agent is ever
  told an issue number exists; a spec change surfaced later is appended to
  the issue the same way (`spec-author` drafts the comment file, orchestrator
  posts it).
- **One git worktree per issue per agent**, `.claude/worktrees/<slug>` —
  inside project dir (agent-reachable), gitignored. Two agents in one
  checkout interleave commits — a branch is not a working tree. Created only
  once gate 2 is approved (first thing to touch disk); removed after merge.
- Plan is a contract. Wrong plan → implementer stops and reports, never
  improvises a different design.
- **The planner never implements.** Planning and implementing are different
  jobs on different models — the plan is drafted by the stronger model, then
  executed by a cheaper one that only has to follow it. Sequential → one
  fresh `spec-implementer` for the whole run, stages in plan order. Parallel
  → one fresh `spec-implementer` per stage (concurrent, separate worktrees).
  Either way the implementer holds none of the planner's exploration, which
  is why the plan's refs must be lossless.
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

### Verify before an approval stop

By running, never by reading:
- **Plan / spec** — a fresh `spec-reviewer` in `plan` scope checks the plan
  against `spec-diff.md` (quoted text matches file, appended IDs unused,
  nothing contradicts an existing requirement, proposes what was asked) and
  writes `review.md`; orchestrator forwards its one-line status.
- **Implementation** — `sh .claude/scripts/gauntlet.sh <worktree>
  artifacts/<slug>` in the agent's worktree: one status line in context,
  full output in `artifacts/<slug>/gauntlet.log`. Code reading, ID-citation
  placement, mutation checks on after-the-fact-looking tests, spec-vs-approved
  diff: `spec-reviewer`'s job, reported in `review.md`.

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
2026-01-02T14:12 gauntlet=pass
```

Log line = `<ISO minute> <event> [key=value …]`; the implementer's own file
carries the stage-event vocabulary.

Parent frontmatter: `issue`, `branch`, `mode: sequential|parallel(N)`,
`gate1`/`gate2` approval dates, current `wave`, `artifacts`. Never the goal or
normative text — issue holds the goal, `artifacts/` holds the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review | reviewed + merged + orchestrator-verified |
| wave gate | not started | stages running | all done; reviewing wave diff | clean — next wave unblocks |
| parent | gate 1 pending | implementing | gate 3/4 | PR squash-merged |

Rules:
- **No agent writes its own `done`.** Implementer stops at `inreview`.
  A fresh `spec-reviewer` reviews the stage, the orchestrator merges and runs
  `gauntlet.sh`, only then moves to `done` — same self-report rule as
  everywhere else.
- **Runnable** = every `blocked-by` id in `done/`. Stage `done` = merged into
  the feature branch ("the code I depend on is on the branch I branch from").
- No `blocked/` directory — blocking derives from `blocked-by`, stated once.
- Card is evidence of intent, never fact. Git is fact. See *Resume*.
- `done/` is a resting spot for a run in progress, not a permanent record — every card for the run is deleted once the PR is merged. See *Merge*.

### Agent hand-off

Every agent writes its full output to a file and ends its turn with a
**status line** — nothing else. The orchestrator's context holds status
lines, paths, and card moves; never a plan, diff, spec, or review body.

```
status=<token> file=<path> [stage=s<n>] [question=<one line>] [reason=<one line>] [count=<n>]
```

| Agent | `status` tokens | `file` |
|---|---|---|
| spec-author | `question` · `ready` · `no-diff` · `reuse` · `new` | `artifacts/<slug>/spec-diff.md`, `issue.md`, `issue-comment.md`, `pr.md` |
| spec-planner | `question` · `spec-gap` · `ready` (`count=` stages) | `artifacts/<slug>/plan.md` |
| spec-implementer | `inreview` · `committed` · `blocked` · `spec-gap` | stage card (`stage=`) |
| spec-reviewer | `clean` · `findings` (`count=` blockers, `stage=` list) | `artifacts/<slug>/review.md` |

`question` = one decision for the user, `question=` carries it verbatim;
the orchestrator relays the user's answer back to the **same** agent
(`SendMessage`), never respawns. Any agent that returns more than the status
line is told to move the rest into its file and answer again. The user
reads a file via its path (`extract-section.sh` for one heading); the
orchestrator never pastes file content into the conversation.

Reviewer scope tokens the orchestrator passes in: `plan` (gate 2 check),
`stage s<n>` (per-stage), `wave w<n>`, `branch` (gate 3).

### Gate 1 — spec diff. Stop for approval.

`spec-author` drafts, user decides, orchestrator relays. Spawn `spec-author`
with: the user's goal verbatim, the affected area(s), `artifacts/<slug>/`.

- **No implementation detail.** No code reading, no code-vs-spec check here
  — the dialog outcome decides that. `status=no-diff` (spec already covers
  it) → the agent's file names the violated requirement; continue to gate 2.
- The agent surfaces every silent decision (scope, defaults, naming, in/out)
  one at a time as `status=question`, with a recommendation; the orchestrator
  relays question and answer without comment.
- Output is the normative text itself: "shall" statements + appended IDs,
  plus `edge-cases.md` entries. Ready to land, not prose about intent.
  Observable design is spec: public signatures, error enum, feature gating,
  config keys.
- `status=ready` → orchestrator points the user at `spec-diff.md`. Change
  requested → relay to the same agent. Approved → record `gate1` on parent
  card; the agent stays alive for gate 1b.
- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning
  — no worktree yet, nothing to put in one.
- **`spec-diff.md` shape:** one `## <ID>` heading per new or changed
  requirement (its full normative text under it, old → new if changed), then
  one `## Other spec changes` heading for `edge-cases.md`/
  `api-contract.md`/`data-contract.md` entries that carry no single ID. Same
  reason `plan.md` is headed per stage:
  `.claude/scripts/extract-section.sh '## <ID>' artifacts/<slug>/spec-diff.md`
  lets a wave-scoped reviewer pull only the IDs its stages touch, never the
  whole file.

### Gate 1b — tracking issue. Stop for approval.

Orchestrator searches existing issues (`gh issue list --state all`) for the
same goal; a candidate's body is read by `spec-author`, not the orchestrator
— pass the number, it runs `bash .claude/scripts/issue-view.sh <number>`
(never raw `gh issue view`) and answers `status=reuse` or `status=new`.
Reuse, never duplicate.

`status=new` → the same `spec-author` writes `artifacts/<slug>/issue.md`:
first line the title, rest the body. Self-contained: full normative text of
every new requirement beside its ID, every changed one old → new, plus
`api-contract.md`/`edge-cases.md` entries; goal and normative changes only,
never file/function/approach. `##` sections `Background`/`Why`, `Scope`,
`Goal`; compact ID ranges; plain-language title. User approves the file →
orchestrator files it:

```sh
gh issue create --title "$(head -1 artifacts/<slug>/issue.md)" --body-file <(tail -n +2 artifacts/<slug>/issue.md)
```

Record `issue` on the parent card. Neither planner nor implementer is ever
told it exists. **Never edit the issue body after filing** — a later spec
change is `spec-author` → `issue-comment.md` → `gh issue comment --body-file`.
An edited body destroys what was originally filed vs. refined later.

### Gate 2 — implementation plan. Stop for approval.

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md`'s path, affected
area(s), anything user volunteered at gate 1. Nothing else — gate 1 did no
code research; agent explores the repo itself.

Writes `plan.md` (shape: `spec-planner.md`'s `## Output`; always opens with
`## Stage s0: land spec`, the stage that copies `spec-diff.md` into
`docs/specs/`) — verification methods for this project: unit tests alone /
driving the demo TUI / a real CSMS, plus expected coverage impact. Any later
reader — implementer, reviewer, resumed session — pulls exactly one section
with `sh .claude/scripts/extract-section.sh '## Stage s<n>: <name>'
artifacts/<slug>/plan.md`, never the whole file.

`status=question` → relay, resume same agent. `status=spec-gap` (approved
text doesn't cover something the plan needs) → agent stays paused;
orchestrator reopens gate 1 scoped to the gap: `spec-author` amends
`spec-diff.md` and drafts `issue-comment.md`, user approves, orchestrator
posts the comment, then resumes the *same* planner (never respawns).

`status=ready` → fresh `spec-reviewer`, scope `plan`. `clean` → point the
user at `plan.md` for approval. `findings` → resume the planner with
`review.md`'s path; re-review; only a clean plan reaches the user.

The plan's dependency tree (`spec-planner.md`'s `## Output` defines it)
reads as waves: a stage is runnable once its dependencies merge. A fully
sequential tree is a normal outcome, not a decomposition failure.

Approval also settles **how it's implemented** — unanswered = not approved:
- **Sequential** — one fresh `spec-implementer` runs every stage in plan
  order. The planner is done at gate 2 either way; it never implements.
- **Parallel** — user gives max concurrent agents; fresh implementer per
  stage. Waves capped at that number.

Default sequential on no preference. Never infer concurrency from plan shape
— five parallel-capable stages doesn't authorize five agents.

**On approval, in order:**
1. Create the worktree — first thing to touch disk in the run:
   `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
2. Record `gate2`+`mode` on parent card, move parent → `inprogress/`.
3. Create `open/<slug>.s<n>.md` per stage (`s0` included), `files`/
   `blocked-by` copied from the tree — `sh .claude/scripts/extract-section.sh
   '## Shared' artifacts/<slug>/plan.md` is the one plan read the
   orchestrator makes, for exactly those two fields. Parallel: also
   `open/<slug>.w<n>.md` per wave. Sequential: one wave-gate card for the
   whole run. Stage ids match plan ids.
4. Spawn the implementer. Its first stage, `s0`, lands the approved spec
   text — normative only, nothing unfinished — as the branch's first commit.
   The orchestrator never touches `docs/specs/`.

### Implement, stage by stage

Sequential: one fresh `spec-implementer`, spawned with the worktree path, the
plan path and its stage cards — never the planner continued. It reads its own
rules (`.claude/agents/spec-implementer.md`, `.claude/AGENTS.core.md`) and
works the stages in plan order, pulling one plan section at a time. Moves
stage card `open`→`inprogress` on start. On green, stage card → `inreview`,
`status=inreview stage=s<n>`; orchestrator runs `gauntlet.sh` and the
per-stage review below. Only once both are clean does the user get the
approval stop; only after approval does the same implementer (resumed)
commit the stage and answer `status=committed`. Push is never the
implementer's — orchestrator pushes the updated worktree to remote once
committed, re-runs `gauntlet.sh` on the pushed sha, moves to `done` —
nothing to merge, so `done` = approved+reviewed+committed+pushed+green.

**Per-stage review** (sequential; parallel's equivalent is the wave gate in
step 5 below). Every green stage gets a **fresh `spec-reviewer`** before its
approval stop — never the implementer, never a resumed reviewer carrying the
previous stage's context. Base ref = the previous stage's commit, or the
branch point for the first stage; scope = that one stage id. Same four axes as
gate 3, on one stage's diff. `clean` → forward it with the gauntlet line at
the approval stop. `findings` → card back to `inprogress/`, resume the
implementer with `review.md`'s path and the stage id (it fixes; the
orchestrator never does), re-run `gauntlet.sh`, fresh reviewer; the approval
stop only happens on a clean review, so an unreviewed stage is never
committed.

This does not replace gate 3. Per-stage review catches a stage's own defects
while the stage is cheap to change; gate 3 reads the whole branch at once and
is the only pass that can see cross-stage bugs, spec drift across stages, and
scope creep that no single stage's diff reveals.

Parallel: one worktree+branch per agent, branched off the feature branch's
current tip at wave start — already containing every earlier wave's merged
stages, so a dependent stage's worktree never starts without its
dependencies' code physically present —
`git worktree add .claude/worktrees/<slug>-<n> -b <type>/<slug>-<n> <type>/<slug>`
— one **fresh** implementer each; concurrent agents can't share a running
context. Never two agents, one worktree. Each wave:

1. Runnable stage cards (`blocked-by` all in `done/`) up to approved count.
   Wave-gate card → `inprogress/`.
2. One implementer per stage: its worktree path, its stage only, its own
   card's absolute path. It commits its stage on green in its own worktree
   (a merge needs a commit) — that commit is still unreviewed and never
   leaves the worktree until step 4 clears it.
3. Wait for the whole wave; each card lands in `inreview/`.
4. Per card: `gauntlet.sh` in its worktree, fresh `spec-reviewer` in
   `stage s<n>` scope (base = the wave's branch point). `clean` → merge into
   the feature branch — **this merge is the new base**, the commit every
   later wave's worktrees branch from — `gauntlet.sh` on the merged result,
   card → `done/`, remove worktree. `findings` → card → `inprogress/`, same
   implementer resumes with `review.md`'s path, fixes, amends its stage
   commit, back to this step. Nothing unreviewed is ever merged.
5. All `done` → wave gate → `inreview/`: fresh `spec-reviewer`, scope
   `wave w<n>`. `clean` → wave gate `done/`, **stop: ask the user for
   approval before the next wave starts**. `findings` → stop, forward the
   status line, implicated stage cards → `inprogress/`; a fresh implementer
   per card gets `review.md`'s path.

A clean wave gate still stops for approval — the merge that closed this wave
is the new base the next wave's worktrees get built on, a fresh checkpoint
even though gate 2 already approved the stages themselves. A finding, red
gauntlet, or merge conflict stops it too. Per-stage approval in sequential
mode and the wave-gate approval in parallel mode are the same checkpoint at
different granularity: nothing lands on the feature branch the user hasn't
seen a clean review for.

Merge conflict between two stages in a wave = dependency tree was wrong —
report, fix the tree, never hand-resolve and continue. Mid-wave stop stops
that wave only: finished branches still merge, the rest re-plans.

Gates unchanged under parallelism — verification, review, spec reconcile all
happen once, on the merged feature branch, never per agent.

- TDD order above. Stage = green checkpoint: builds, tests pass, lint clean,
  coverage ≥ 80%. A green stage is independently reviewed, then pauses for
  approval; only after approval does the implementer commit the stage. Push is never the implementer's — only
  the orchestrator pushes the updated worktree to remote, after that same
  approval — makes the plan resumable.
- Stage messages cheap, squashed later. Squash message carries requirement
  IDs + why, body hard-wrapped at 72 — the one text in this workflow that
  *is* wrapped (`git log` never soft-wraps); spec, issue, PR text never. Spec = first stage, first commit.
- **Never add `Co-Authored-By`, "Generated with," or any tool attribution
  trailer** to a commit, PR body, issue, or comment — no information, pure
  `git log` noise, forever. Every agent, every gate, squash message and PR
  body included; `hook-guard-attribution.sh` denies the command anyway.
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

Implementer returns `status=spec-gap reason=…` when behavior must differ
from gate 1 approval — normative, **reopens gate 1**: `spec-author` amends
`spec-diff.md` (old → new, what forced it) and drafts `issue-comment.md`;
user approves; orchestrator posts the comment; implementer resumes and lands
the amended text in `docs/specs/` before continuing. Wrong cross-reference /
clumsy wording = editorial, the implementer fixes it in place, no approval.
Gate 3's reviewer diffs the branch's spec against `spec-diff.md` — that is
the final spec report, never one the orchestrator composes.

### Gate 3 — review. Stop for approval.

Before proposing a PR: fresh `spec-reviewer`, scope `branch` (a reviewer
sharing the implementer's context reproduces its blind spots). Stages were
already reviewed one at a time; this is the whole-branch pass, the only one
that can catch cross-stage bugs, spec drift across stages, and scope creep
no single stage's diff shows. Give it the base ref, the artifact dir, the
worktree path, and the stage ids in scope (all of them). It reads its own
rules (`.claude/AGENTS.core.md`) itself. Four axes — spec fidelity, standards,
TDD honesty, docs currency; full criteria: `spec-reviewer.md`'s `## Four
axes, reported separately`.

Orchestrator runs `gauntlet.sh` on the branch and forwards both status
lines. `findings` → implicated cards → `inprogress/`, fresh implementer with
`review.md`'s path, re-gauntlet, fresh reviewer. Findings flagged as needing
a user decision are relayed as questions, never fixed by anyone unasked.

**Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage
id so the right cards return to `inprogress/`. Parent card → `inreview/`
when review starts.

### Gate 4 — pull request. Stop for approval.

- Gauntlet + gate 3 clean, then **ask whether to open a PR** — user may
  want a manual run first; don't pre-empt it.
- `spec-author` writes `artifacts/<slug>/pr.md` (first line title, rest
  body) from `spec-diff.md`, `plan.md`, `review.md`, `gauntlet.log` and
  `git log main..HEAD`. User approves the file; orchestrator pushes and
  opens: `gh pr create --title "$(head -1 artifacts/<slug>/pr.md)"
  --body-file <(tail -n +2 artifacts/<slug>/pr.md)`.
- CI fails on the pushed branch → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`, to see the failure.
- Reading an existing PR's title/body/comments (e.g. gate 3 review context, or checking for feedback after pushing) → `bash .claude/scripts/pr-view.sh <number>`, never raw `gh pr view`.
- Title plain language, issue's style. Body has four sections, in order —
  Why, What changed, Approach, Verification — dropping one only when
  genuinely inapplicable: why (requirement IDs, motivation), what changed,
  approach (how the issue was resolved, structure it omitted), verification
  (what was actually run, ending with the current coverage percentage), and
  `Closes #<issue>` — the one line the orchestrator appends itself, since
  only it knows the number.

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

Merged and worktrees clean → **delete every card for this run** (stage cards,
wave-gate cards, parent card): `done/` was only ever a resting spot, never
the archive. Then ask the user for final "work done" approval — a distinct
question from gate 4's PR approval. Approved → also delete
`artifacts/<slug>/` (`spec-diff.md`, `plan.md`, `review.md`) for a clean
slate. Declined → leave cards and artifacts in place; whatever prompted the
decline gets sorted out before either is removed.

### Resume an interrupted run

Cards outside `open/`+`done/`, no agent running = session died mid-run.
Resume triggered by the user or `/spec-feature`, never automatic.

No worktree recorded on the card → died during gate 1 dialog or gate 2
planning, nothing on disk to reconcile — resume the conversation from
`spec-diff.md`/`plan.md`'s last state. Past gate 2 → table below either way.
**Any resumed implementation spawns a fresh agent** — same as a first run;
no implementer context survives a crash. This is why plan refs must be
lossless: an implementer resuming mid-plan gets nothing but what the plan
wrote down.

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `gauntlet=pass` | `gauntlet.sh` at that sha | card overstated state |
| stage `done` | `git branch --contains` vs feature branch | never merged; downstream plans a lie |

Report differences first. Agree → resume. Disagree → stop and report: card
behind git is a forgotten move, correctable; card claiming what git can't
show is never trusted into being true.

Clean reconcile → resume only no-approval work (respawn implementers for
approved stages, merge finished branches, run wave gates); halt at the first
gate needing the user. Recorded `gate1`/`gate2` approvals stay valid, no
re-ask.
