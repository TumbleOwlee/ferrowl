# AGENTS.workflow.md

Gate/task-board mechanics, split out of `AGENTS.md` (`## Workflow` pointer) so sessions that never run the workflow don't pay for it. Orchestrator only — `AGENTS.core.md` readers (spec-author/planner/implementer/reviewer) never read this file.

## Workflow

Trigger: `AGENTS.md`'s `## Workflow`. Every heading below is one `extract-section.sh` pull; never read whole.

### Principles

- Replaces any generic workflow skill (`/workflow`); `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`, `fix`, `docs`}. **Enforced:** `.claude/scripts/hook-guard-shell.sh` denies `git commit` on `main` and `git push` targeting `main`.
- **Orchestrator never changes spec or code and never reads either.** It spawns agents, relays one question at a time between agent and user, runs git/`gh` plumbing (worktree add/remove, merge, push, `gh … --body-file`), runs `.claude/scripts/gauntlet.sh`, moves cards. Every other output — spec text, issue body, plan, code, review, PR body — is a file an agent wrote under `artifacts/<slug>/` or in a worktree. Orchestrator hands paths around, never contents; a file needing user approval is opened with `.claude/scripts/show-file.sh`, never pasted. **Two outputs per agent:** the full file the next agent consumes, and a capped summary the user approves (`plan.summary.md`, `review.verdict.md`); the user sees the summary, never the full file. See *Agent hand-off*.
- **Gate 1 is a dialog with the user, drafted by `spec-author`.** Existing spec + goal, nothing about current code. No worktree/branch until gate 2 approved.
- **No spec effect (docs-only, non-functional, no observable-behavior change) → skip gate 1**, go to gate 2. No `spec-diff.md`, no `gate1` on the parent card. Gate 2 planning finds a spec gap → stop, run gate 1, continue.
- Gate 2 onward delegates to agents. **All agents Sonnet or better.**
- **Issue and PR filed by the orchestrator alone**, from a body file `spec-author` drafted. Planner and implementer never learn an issue number exists. A later spec change goes to the issue the same way (`spec-author` drafts the comment file, orchestrator posts).
- **One git worktree per issue per agent**, `.claude/worktrees/<slug>` — inside project dir (agent-reachable), gitignored. Two agents in one checkout interleave commits; a branch is not a working tree. Created only once gate 2 is approved (first thing to touch disk); removed after merge.
- Plan is a contract; a wrong plan stops the implementer (`spec-implementer.md` `## Stop and report`), never gets improvised around.
- **Planner never implements.** Plan drafted by the stronger model, executed by a cheaper one. Sequential → one fresh `spec-implementer` for the run, stages in plan order. Parallel → one fresh `spec-implementer` per stage, separate worktrees. Implementer holds none of the planner's exploration, so plan refs must be lossless.
- **An agent's self-reported verification is not verification.** Re-run the tools.
- **Every state change moves a task-board card.** State living only in conversation dies with the session.
- **Area `requirements.md`/`edge-cases.md` large enough that reading costs real context: propose a split** at gate 1, before drafting. Split along a real sub-capability seam, never a line-count cut. New prefix for the new sub-area; **moved requirements keep their original ID** (IDs are cited in tests). Only requirements added after the split take the new prefix. Routing table updated. User approves; never silent.

### Verify before an approval stop

By running, never by reading:
- **Plan / spec** — fresh `spec-reviewer`, scope `plan`, checks plan against `spec-diff.md` (quoted text matches file, appended IDs unused, nothing contradicts an existing requirement, proposes what was asked), writes `review.md`; orchestrator forwards its status line.
- **Implementation** — `sh .claude/scripts/gauntlet.sh <worktree> artifacts/<slug>` in the agent's worktree: one status line in context, full output in `artifacts/<slug>/gauntlet.log`. Code reading, ID-citation placement, mutation checks on after-the-fact-looking tests, spec-vs-approved diff: `spec-reviewer`'s job, in `review.md`.

### Task board

State on disk so an interrupted session resumes, not restarts. Directory a card sits in **is** its state:

```
.claude/tasks/
  open/  inprogress/  inreview/  done/   cards move between these
  artifacts/<slug>/
    spec-diff.md   gate 1 approved normative text
    plan.md            gate 2 stages, steps, dependency tree — the implementer's contract
    plan.summary.md    gate 2 decisions the user approves, capped
    review.md          review findings + history, keyed by stage id, append-only
    review.verdict.md  open findings per stage, rewritten every pass, capped
```

Directories tracked; cards gitignored. Cards live in **main checkout only** — a worktree agent gets its own card's absolute path, writes only that file. No per-worktree board copy.

| Card | File | Owner |
|---|---|---|
| parent | `<slug>.md` | orchestrator — one per run |
| stage | `<slug>.s<n>.md` | the implementer working that stage |
| wave gate | `<slug>.w<n>.md` | orchestrator — one per parallel wave |

Cards are agent-only: YAML frontmatter + append-only log, terse field=value tokens, no prose. Agents **append**, never rewrite a line — crash mid-write costs one truncated line, not the file.

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

Log line = `<ISO minute> <event> [key=value …]`; the implementer's own file carries the stage-event vocabulary.

Parent frontmatter: `issue`, `branch`, `mode: sequential|parallel(N)`, `gate1`/`gate2` approval dates, current `wave`, `artifacts`. Never the goal or normative text — issue holds the goal, `artifacts/` the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review | reviewed + merged + orchestrator-verified |
| wave gate | not started | stages running | all done; reviewing wave diff | clean — next wave unblocks |
| parent | gate 1 pending | implementing | gate 3/4 | PR squash-merged |

Rules:
- **No agent writes its own `done`.** Implementer stops at `inreview`. Fresh `spec-reviewer` reviews, orchestrator merges and runs `gauntlet.sh`, only then `done`.
- **Runnable** = every `blocked-by` id in `done/`. Stage `done` = merged into the feature branch.
- No `blocked/` directory — blocking derives from `blocked-by`.
- Card is intent, never fact. Git is fact. See *Resume*.
- `done/` is a resting spot during a run, not a record — every card for the run is deleted once the PR merges. See *Merge*.

### Agent hand-off

Every agent writes its full output to a file and ends its turn with a **status line** — nothing else. Orchestrator context holds status lines, paths, card moves; never a plan, diff, spec, or review body.

```
status=<token> file=<path> [summary=<path>] [stage=s<n>] [question=<one line>] [reason=<one line>] [count=<n>]
```

| Agent | `status` tokens | `file` | `summary` |
|---|---|---|---|
| spec-author | `question` · `ready` · `no-diff` · `reuse` · `new` | `artifacts/<slug>/spec-diff.md`, `issue.md`, `issue-comment.md`, `pr.md` | none — normative lines are already the summary |
| spec-planner | `question` · `spec-gap` · `ready` (`count=` stages) | `artifacts/<slug>/plan.md` | `artifacts/<slug>/plan.summary.md` |
| spec-implementer | `inreview` · `committed` · `blocked` · `spec-gap` | stage card (`stage=`) | none — the card is the summary |
| spec-reviewer | `clean` · `findings` (`count=` blockers+majors, `stage=` list) | `artifacts/<slug>/review.md` | `artifacts/<slug>/review.verdict.md` |

**Two outputs, two readers.** `file` is lossless, for the next agent: as long as it must be. `summary` is for the user: decisions and open findings only, rewritten whole on every pass, capped at 25 lines / 2 KB (`show-file.sh` refuses a larger one and exits 3 — the agent trims it; the orchestrator never opens the full file in its place). Each rule applies to its own file only: a summary that explains *how* is bloat, a full file that skips a reference is incomplete.

`question` = one decision for the user, carried verbatim in `question=`; orchestrator relays the answer to the **same** agent (`SendMessage`), never respawns. An agent returning more than the status line is told to move the rest into its file and answer again. When a status line names a file the user must approve (`spec-diff.md`, `issue.md`, `pr.md`, and the `summary=` path — `plan.summary.md`, `review.verdict.md`), orchestrator runs `sh .claude/scripts/show-file.sh <path>` before asking — opens the file in a viewer outside the context (tmux+glow, wslview, tmux+less, or a manual hint), prints one line, never the content. The approval prompt itself is one line: gate, slug, the status line's counts, the answers accepted. User pulls one heading of the full file with `extract-section.sh` when they want the *why*; orchestrator never pastes file content.

Reviewer scope tokens: `plan` (gate 2), `stage s<n>`, `wave w<n>`, `branch` (gate 3).

### Gate 1 — spec diff. Stop for approval.

`spec-author` drafts, user decides, orchestrator relays. Spawn `spec-author` with: user's goal verbatim, affected area(s), `artifacts/<slug>/`.

- **No implementation detail.** No code reading, no code-vs-spec check. `status=no-diff` (spec already covers it) → agent's file names the violated requirement; continue to gate 2.
- Agent surfaces every silent decision (scope, defaults, naming, in/out) one at a time as `status=question` with a recommendation; orchestrator relays question and answer without comment.
- Output is the normative text: "shall" statements + appended IDs, plus `edge-cases.md` entries. Ready to land, not prose about intent. Observable design is spec: public signatures, error enum, feature gating, config keys.
- `status=ready` → point user at `spec-diff.md`. Change requested → relay to the same agent. Approved → record `gate1` on parent card; agent stays alive for gate 1b.
- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning — no worktree yet.
- **`spec-diff.md` shape:** one `## <ID>` heading per new/changed requirement (full normative text under it, old → new if changed), then `## Other spec changes` for `api-contract.md`/`data-contract.md` changes with no single owning ID (every `edge-cases.md` entry carries its own `-E` ID and gets its own `## <ID>` heading like a requirement). `.claude/scripts/extract-section.sh '## <ID>' artifacts/<slug>/spec-diff.md` lets a wave-scoped reviewer pull only its IDs.

### Gate 1b — tracking issue. Stop for approval.

Orchestrator searches existing issues (`gh issue list --state all`) for the same goal; a candidate's body is read by `spec-author`, not the orchestrator — pass the number, it runs `bash .claude/scripts/issue-view.sh <number>` (never raw `gh issue view`) and answers `status=reuse` or `status=new`. Reuse, never duplicate.

`status=new` → same `spec-author` writes `artifacts/<slug>/issue.md`: line 1 title, rest body. Self-contained: full normative text of every new requirement beside its ID, every changed one old → new, plus `api-contract.md`/`edge-cases.md` entries; goal and normative changes only, never file/function/approach. `##` sections `Background`/`Why`, `Scope`, `Goal`; compact ID ranges; plain-language title. User approves → orchestrator files:

```sh
gh issue create --title "$(head -1 artifacts/<slug>/issue.md)" --body-file <(tail -n +2 artifacts/<slug>/issue.md)
```

Record `issue` on the parent card. Planner and implementer never told it exists. **Never edit the issue body after filing** — a later spec change is `spec-author` → `issue-comment.md` → `gh issue comment --body-file`. An edited body destroys the originally-filed vs refined-later record.

### Gate 2 — implementation plan. Stop for approval.

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md` path, affected area(s), anything the user volunteered at gate 1. Nothing else — gate 1 did no code research; the agent explores the repo itself.

Writes `plan.md` (shape: `spec-planner.md`'s `## Output`; always opens with `## Stage s0: land spec`, copying `spec-diff.md` into `docs/specs/`) — verification methods for this project: unit tests alone / driving the demo TUI / a real CSMS, plus expected coverage impact. Any later reader pulls one section: `sh .claude/scripts/extract-section.sh '## Stage s<n>: <name>' artifacts/<slug>/plan.md`.

`status=question` → relay, resume same agent. `status=spec-gap` (approved text doesn't cover something the plan needs) → agent stays paused; orchestrator reopens gate 1 scoped to the gap: `spec-author` amends `spec-diff.md` and drafts `issue-comment.md`, user approves, orchestrator posts, resumes the *same* planner.

`status=ready` → fresh `spec-reviewer`, scope `plan`. `clean`, or `findings` with `count=0` → show `plan.summary.md`, then `review.verdict.md` if it lists minors, and ask. `count>0` → resume planner with `review.md` path; re-review. Minors never loop: the user sees them on the verdict and decides at the same stop.

The plan's dependency tree (`spec-planner.md`'s `## Output`) reads as waves: a stage is runnable once its dependencies merge. A fully sequential tree is normal, not a failure.

Approval also settles **how** — unanswered = not approved:
- **Sequential** — one fresh `spec-implementer` runs every stage in plan order. Planner is done at gate 2 either way.
- **Parallel** — user gives max concurrent agents; fresh implementer per stage. Waves capped at that number.

Default sequential. Never infer concurrency from plan shape.

**On approval, in order:**
1. Create the worktree — first thing to touch disk: `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
2. Record `gate2`+`mode` on parent card, move parent → `inprogress/`.
3. Create `open/<slug>.s<n>.md` per stage (`s0` included), `files`/`blocked-by` copied from the tree — `sh .claude/scripts/extract-section.sh '## Shared' artifacts/<slug>/plan.md` is the one plan read the orchestrator makes, for exactly those two fields. Parallel: also `open/<slug>.w<n>.md` per wave. Sequential: one wave-gate card for the run. Stage ids match plan ids.
4. Spawn the implementer. Its first stage, `s0`, lands the approved spec text — normative only — as the branch's first commit. Orchestrator never touches `docs/specs/`.

### Implement, stage by stage

Sequential: one fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards — never the planner continued. It reads its own rules (`.claude/agents/spec-implementer.md`, `.claude/AGENTS.core.md`) and works stages in plan order, one plan section at a time. Stage card `open`→`inprogress` on start. On green: card → `inreview`, `status=inreview stage=s<n>`; orchestrator runs `gauntlet.sh` and the per-stage review below. Both clean → user approval stop; after approval the same implementer (resumed) commits and answers `status=committed`. Push is never the implementer's — orchestrator pushes the worktree once committed, re-runs `gauntlet.sh` on the pushed sha, moves to `done` (nothing to merge: `done` = approved+reviewed+committed+pushed+green).

**Per-stage review** (sequential; parallel's equivalent is the wave gate, step 5 below). Every green stage gets a **fresh `spec-reviewer`** before its approval stop — never the implementer, never a resumed reviewer. Base ref = previous stage's commit, or branch point for the first stage; scope = that stage id. Same four axes as gate 3, on one stage's diff. `clean`, or `findings` with `count=0` → show `review.verdict.md`, forward the gauntlet line, approval stop; minors on the verdict are the user's call there, never a loop. `count>0` → card back to `inprogress/`, resume implementer with `review.md` path and stage id (it fixes; orchestrator never does), re-run `gauntlet.sh`, fresh reviewer. An unreviewed stage is never committed.

Not a replacement for gate 3: per-stage review catches a stage's own defects while cheap to change; gate 3 reads the whole branch, the only pass seeing cross-stage bugs, spec drift across stages, and scope creep no single diff reveals.

Parallel: one worktree+branch per agent, branched off the feature branch's tip at wave start (already containing every earlier wave's merged stages, so a dependent stage's worktree never starts without its dependencies' code): `git worktree add .claude/worktrees/<slug>-<n> -b <type>/<slug>-<n> <type>/<slug>`. One **fresh** implementer each; never two agents in one worktree. Each wave:

1. Runnable stage cards (`blocked-by` all in `done/`) up to approved count. Wave-gate card → `inprogress/`.
2. One implementer per stage: its worktree path, its stage only, its own card's absolute path. It commits its stage on green in its own worktree (a merge needs a commit) — unreviewed, never leaves the worktree until step 4 clears it.
3. Wait for the whole wave; each card lands in `inreview/`.
4. Per card: `gauntlet.sh` in its worktree, fresh `spec-reviewer` in `stage s<n>` scope (base = wave's branch point). `clean` → merge into feature branch — **this merge is the new base** every later wave branches from — `gauntlet.sh` on the merged result, card → `done/`, remove worktree. `findings` → card → `inprogress/`, same implementer resumes with `review.md` path, fixes, amends its stage commit, back to this step. Nothing unreviewed is merged.
5. All `done` → wave gate → `inreview/`: fresh `spec-reviewer`, scope `wave w<n>`. `clean` → wave gate `done/`, **stop: ask the user for approval before the next wave**. `findings` → stop, forward the status line, implicated stage cards → `inprogress/`; fresh implementer per card gets `review.md` path.

A clean wave gate still stops for approval — its merge is the next wave's base, a fresh checkpoint. A finding, red gauntlet, or merge conflict stops it too. Per-stage approval (sequential) and wave-gate approval (parallel) are the same checkpoint at different granularity: nothing lands on the feature branch without a clean review the user has seen.

Merge conflict between two stages in a wave = dependency tree was wrong — report, fix the tree, never hand-resolve and continue. Mid-wave stop stops that wave only: finished branches still merge, the rest re-plans.

Gates unchanged under parallelism — verification, review, spec reconcile happen once, on the merged feature branch, never per agent.

- TDD order above. Stage = green checkpoint: builds, tests pass, lint clean, coverage ≥ 80%. Reviewed, then approval, then the implementer commits. Push only by the orchestrator, after that approval — keeps the plan resumable.
- Stage messages cheap, squashed later. Squash message carries requirement IDs + why, body hard-wrapped at 72 — the one wrapped text in this workflow; spec, issue, PR text never. Spec = first stage, first commit.
- **Never add `Co-Authored-By`, "Generated with", or any tool attribution trailer** to a commit, PR body, issue, or comment — pure `git log` noise, forever. Every agent, every gate, squash message and PR body included; `hook-guard-attribution.sh` denies it anyway.
- Every new/changed requirement ships ≥1 ID-citing test.
- Every existing test pinning observable behavior cites its requirement. Pure internal/helper-detail tests may stay untagged. Behavior no requirement states = requirement missing — add it (gate 1), never attach a loose ID.
- Citation directly beside the test declaration, above the function body. ≤1 ID per test.
- Not done until the Verification method has run and its outcome is reported. Waiving it requires asking.

### Reconcile the spec

Implementer returns `status=spec-gap reason=…` when behavior must differ from gate 1 approval — normative, **reopens gate 1**: `spec-author` amends `spec-diff.md` (old → new, what forced it) and drafts `issue-comment.md`; user approves; orchestrator posts; implementer resumes and lands the amended text in `docs/specs/` before continuing. Wrong cross-reference / clumsy wording = editorial, implementer fixes in place, no approval. Gate 3's reviewer diffs the branch's spec against `spec-diff.md` — the final spec report, never one the orchestrator composes.

### Gate 3 — review. Stop for approval.

Before proposing a PR: fresh `spec-reviewer`, scope `branch` (a reviewer sharing the implementer's context reproduces its blind spots). Whole-branch pass: cross-stage bugs, spec drift across stages, scope creep. Give it base ref, artifact dir, worktree path, all stage ids. It reads its own rules (`.claude/AGENTS.core.md`). Four axes — spec fidelity, standards, TDD honesty, docs currency; criteria: `spec-reviewer.md`'s `## Four axes, reported separately`.

Orchestrator runs `gauntlet.sh` on the branch, forwards both status lines, shows `review.verdict.md`. `count>0` → implicated cards → `inprogress/`, fresh implementer with `review.md` path, re-gauntlet, fresh reviewer. `count=0` with minors → the user decides at the stop. Findings needing a user decision are relayed as questions, never fixed unasked.

**Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage id, and rewrites `review.verdict.md`. Parent card → `inreview/` when review starts.

### Gate 4 — pull request. Stop for approval.

- Gauntlet + gate 3 clean, then **ask whether to open a PR** — user may want a manual run first.
- `spec-author` writes `artifacts/<slug>/pr.md` (line 1 title, rest body) from `spec-diff.md`, `plan.md`, `review.md`, `gauntlet.log`, `git log main..HEAD`. User approves; orchestrator pushes and opens: `gh pr create --title "$(head -1 artifacts/<slug>/pr.md)" --body-file <(tail -n +2 artifacts/<slug>/pr.md)`.
- CI fails → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`.
- Reading an existing PR's title/body/comments → `bash .claude/scripts/pr-view.sh <number>`, never raw `gh pr view`.
- Title plain language, issue's style. Body four sections in order — Why (requirement IDs, motivation), What changed, Approach (how resolved, structure it omitted), Verification (what actually ran, ending with current coverage percentage) — dropping one only when genuinely inapplicable — then `Closes #<issue>`, the one line the orchestrator appends itself.

### Merge

Squash merge to `main` — stage commits, including the ahead-of-code spec commit, never reach `main`. Then:

```sh
git worktree remove .claude/worktrees/<slug>
git worktree list   # nothing under .claude/worktrees/ should remain
```

Per-wave worktrees are removed at wave end; this sweep catches stragglers. Parent card → `done/` — no card for this run stays outside `done/`.

Merged and worktrees clean → **delete every card for this run** (stage, wave-gate, parent): `done/` was never the archive. Then ask the user for final "work done" approval — distinct from gate 4's PR approval. Approved → also delete `artifacts/<slug>/`. Declined → leave cards and artifacts; sort out the decline's cause before removing either.

### Resume an interrupted run

Cards outside `open/`+`done/`, no agent running = session died mid-run. Resume triggered by the user or `/spec-feature`, never automatic.

No worktree on the card → died during gate 1 dialog or gate 2 planning, nothing on disk to reconcile — resume the conversation from `spec-diff.md`/`plan.md`'s last state. Past gate 2 → table below. **Any resumed implementation spawns a fresh agent** — no implementer context survives a crash; the plan is all it gets.

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `gauntlet=pass` | `gauntlet.sh` at that sha | card overstated state |
| stage `done` | `git branch --contains` vs feature branch | never merged; downstream plans a lie |

Report differences first. Agree → resume. Disagree → stop and report: card behind git is a forgotten move, correctable; card claiming what git can't show is never trusted into being true.

Clean reconcile → resume only no-approval work (respawn implementers for approved stages, merge finished branches, run wave gates); halt at the first gate needing the user. Recorded `gate1`/`gate2` approvals stay valid.
