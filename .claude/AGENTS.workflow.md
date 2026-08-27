# AGENTS.workflow.md

Gate/task-board workflow mechanics, split out of `AGENTS.md` (see that
file's `## Workflow` pointer) so an eager `AGENTS.md` read doesn't pay
for gate mechanics on sessions that never run the spec-feature workflow.
Orchestrator only — `AGENTS.core.md` readers (spec-planner/implementer/
reviewer agents) never read this file.

## Workflow

Triggers on **behavior change, any size**: new public function, changed
default, new error variant, any observable semantics. Not a behavior change:
refactor, rename, perf-with-identical-semantics, tests, docs — no gates, just
do it. Size sets stage count, never gate existence.

- Replaces any generic workflow skill (`/workflow`) — don't run one.
  `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`,
  `fix`, `docs`}. **Enforced, not just advisory:** the same `PreToolUse` hook
  (`.claude/scripts/hook-guard-cat.sh`) denies `git commit` while the
  checkout is on `main`, and `git push` targeting `main` — the safety net
  for an agent that missed the worktree step, not just a written rule.
- **Gate 1 = orchestrator's own conversation with the user, not an agent's.**
  Abstract: existing spec + current goal, nothing about current code. No
  worktree/branch until gate 2 approved.
- **No spec effect (docs-only, other non-functional change with no
  observable-behavior change) → skip gate 1 entirely**, proceed straight to
  gate 2. No `spec-diff.md`, no `gate1` on the parent card. If gate 2
  planning turns up an actual spec gap, stop and run gate 1 before
  continuing — same as any other spec-gap pause.
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
- `done/` is a resting spot for a run in progress, not a permanent record — every card for the run is deleted once the PR is merged. See *Merge*.

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
- **`spec-diff.md` shape:** one `## <ID>` heading per new or changed
  requirement (its full normative text under it, old → new if changed), then
  one `## Other spec changes` heading for `edge-cases.md`/
  `api-contract.md`/`data-contract.md` entries that carry no single ID. Same
  reason `plan.md` is headed per stage:
  `.claude/scripts/extract-section.sh '## <ID>' artifacts/<slug>/spec-diff.md`
  lets a wave-scoped reviewer pull only the IDs its stages touch, never the
  whole file.

### Gate 1b — tracking issue. Orchestrator runs this itself. Stop for approval.

Search existing issues (`gh issue list`, open and closed) for the same goal;
read any candidate with `bash .claude/scripts/issue-view.sh <number>`, never raw
`gh issue view`. Reuse, never duplicate.

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
**Never edit the issue body after filing** — always `gh issue comment` (or
the Jira equivalent) to append the delta. An edited body destroys the
history of what was originally filed vs. refined later; a comment preserves
that trail and keeps the issue trackable.

### Gate 2 — implementation plan. Stop for approval.

Spawn the planning agent with a brief: approved spec text, affected area(s),
anything user volunteered at gate 1. Nothing else — gate 1 did no code
research; agent explores the repo itself. Never mention the issue.

Returns `plan.md` (shape: `spec-planner.md`'s `## Output`) — verification methods for this project: unit tests alone / driving the demo TUI / a real CSMS, plus expected coverage impact. Any later reader — implementer, reviewer, resumed session — pulls exactly one section with `sh .claude/scripts/extract-section.sh '## Stage s<n>: <name>' artifacts/<slug>/plan.md`, never the whole file.

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
`open`→`inprogress` on start. On green, stage card → `inreview`, stop and
wait for approval before touching git — only after approval does it commit
the stage. Push is never the implementer's — orchestrator pushes the
updated worktree to remote once committed. Orchestrator verifies, moves to
`done` — nothing to merge, so `done` = approved+committed+pushed+verified.

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
  coverage ≥ 80%. Green stage pauses for approval; only after approval does
  the implementer commit the stage. Push is never the implementer's — only
  the orchestrator pushes the updated worktree to remote, after that same
  approval — makes the plan resumable.
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
worktree path, and the stage ids in scope (all of them, at gate 3) — never the
issue number, same as every agent in this workflow. It reads its own rules
(`.claude/AGENTS.core.md`) itself. Reports on four axes — spec fidelity,
standards, TDD honesty, docs currency; full criteria: `spec-reviewer.md`'s
`## Four axes, reported separately`.

Re-run the verification yourself, report findings + fixes. User-decision
findings are raised, not silently fixed.

**Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage
id so the right cards return to `inprogress/`. Parent card → `inreview/`
when review starts.

### Gate 4 — pull request. Stop for approval.

- Verification run + reported, then **ask whether to open a PR** — user may
  want a manual run first; don't pre-empt it.
- Draft title + body, get approval of that text, push, open.
- CI fails on the pushed branch → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`, to see the failure.
- Reading an existing PR's title/body/comments (e.g. gate 3 review context, or checking for feedback after pushing) → `bash .claude/scripts/pr-view.sh <number>`, never raw `gh pr view`.
- Title plain language, issue's style. Body has four sections, in order —
  Why, What changed, Approach, Verification — dropping one only when
  genuinely inapplicable: why (requirement IDs, motivation), what changed,
  approach (how the issue was resolved, structure it omitted), verification
  (what was actually run, ending with the current coverage percentage), and
  `Closes #<issue>`.
- Never add `Co-Authored-By`, "Generated with," or any tool attribution
  trailer to the commit or the PR body — same rule as everywhere else in
  this workflow, restated here because this is the step it's most often
  missed at.

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
