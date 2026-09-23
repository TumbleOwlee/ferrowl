# AGENTS.workflow.md

Gate/task-board mechanics for the orchestrator. Subagents (spec-author/planner/implementer/reviewer) never read it. Every heading below is one `extract-section.sh` pull.

### Principles

- Replaces any generic workflow skill (`/workflow`); `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`, `fix`, `docs`}. **Enforced:** `.claude/scripts/hook-guard-shell.sh` denies `git commit` on `main` and `git push` targeting `main`.
- **Orchestrator never changes spec or code and never reads spec, code, or any agent-written file.** It spawns agents, relays one question at a time between agent and user, runs git/`gh` plumbing (worktree add/remove, merge, push, `gh … --body-file`), runs `.claude/scripts/gauntlet.sh`, moves cards. Every other output is an agent's file (*Agent hand-off*).
- **Gate 1 is a dialog with the user, drafted by `spec-author`.** Existing spec + goal, nothing about current code.
- **Non-behavior change (`AGENTS.md` `## Workflow`) skips gate 1:** no `spec-diff.md`, no `gate1` on the parent card. Gate 2 planning finds a spec gap → stop, run gate 1, continue.
- Gate 2 onward delegates to agents. **All agents Sonnet or better.**
- **Issue and PR filed by the orchestrator alone**, from a body file `spec-author` drafted, title = line 1 with its `# ` stripped, body = the rest, never retyped. No agent but `spec-author` learns an issue or PR number exists — spawn briefs never carry one. A later spec change goes to the issue the same way (`spec-author` drafts the comment file, orchestrator posts).
- **The draft PR is the user's review surface** (opened by the first push, *Implement*; promoted, never replaced, at *Gate 4*). Feedback re-enters the run only through `bash .claude/scripts/pr-feedback.sh` (*PR feedback*): the file it writes goes to the implementer like `review.md`.
- **One git worktree per issue per agent**, `.claude/worktrees/<slug>` — inside project dir (agent-reachable), gitignored. Created only once gate 2 is approved (first thing to touch disk); removed after merge.
- **Plan is a contract; planner never implements.** A wrong plan stops the implementer (`spec-implementer.md` `## Stop and report — never improvise`), never gets improvised around.
- **An agent's self-reported verification is not verification.** Re-run the tools.
- **Every state change moves a task-board card.** State living only in conversation dies with the session.
- **An area whose spec costs real context to read gets a split proposed** by `spec-author` at gate 1 (rules in its file); user approves, never silent.

### Verify before an approval stop

By running, never by reading:
- **Gauntlet** — `sh .claude/scripts/gauntlet.sh <worktree> artifacts/<slug>` in the agent's worktree: one status line in context, full output in `artifacts/<slug>/gauntlet.log`.
- **Every reviewer is a fresh `spec-reviewer`** — never the implementer, never a resumed reviewer: one sharing the implementer's context reproduces its blind spots.
- **Minors never re-trigger the fix-and-review loop.** They stay on the verdict for the user to accept or assign at the approval stop; only `count>0` (blockers + majors) loops.

### Task board

Directory a card sits in **is** its state:

```
.claude/tasks/
  open/  inprogress/  inreview/  done/   cards move between these
  artifacts/<slug>/
    spec-diff.md   gate 1 approved normative text
    plan.md            gate 2 stages, steps, dependency tree — the implementer's contract
    plan.summary.md    gate 2 decisions the user approves, capped
    review.md          review findings + history, keyed by stage id, append-only
    review.verdict.md  open findings per stage, rewritten every pass, capped
    gauntlet.log       full build/test/lint output of the last gauntlet.sh run
    issue.md           `# <title>` (line 1) + body of the tracking issue; issue-comment.md, a later spec amendment
    pr.md              `# <title>` (line 1) + body — draft form after gate 2, full form at gate 4
    pr-feedback.md     unresolved PR review threads (pr-feedback.sh fetch), replies filled by the implementer
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

Parent frontmatter: `issue`, `pr` (number, set when the draft opens), `branch`, `mode: sequential|parallel(N)`, `gate1`/`gate2` approval dates, current `wave`, `artifacts`. Never the goal or normative text — issue holds the goal, `artifacts/` the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review | reviewed + on the feature branch + orchestrator-verified |
| wave gate | not started | stages running | all done; reviewing wave diff | clean — next wave unblocks |
| parent | gate 1 pending | implementing | gate 3/4 | PR squash-merged |

Rules:
- **No agent writes its own `done`.** Implementer stops at `inreview`. `spec-reviewer` reviews, orchestrator merges and runs `gauntlet.sh`, only then `done`.
- **Runnable** = every `blocked-by` id in `done/`. Stage `done` = reviewed, approved, and on the feature branch: pushed (sequential) or merged (parallel).
- No `blocked/` directory — blocking derives from `blocked-by`.
- Card is intent, git is fact — see *Resume*.
- `done/` is not an archive — see *Merge*.

### Agent hand-off

Every agent writes its full output to a file and ends its turn with a **status line** — nothing else.

```
status=<token> [file=<path>] [summary=<path>] [stage=s<n>] [issue=<n>] [question=<one line>] [reason=<one line>] [count=<n>]
```

| Agent | `status` tokens | `file` | `summary` |
|---|---|---|---|
| spec-author | `question` · `ready` · `no-diff` · `reuse` · `new` | `artifacts/<slug>/spec-diff.md`, `issue.md`, `issue-comment.md`, `pr.md` | none — each file opens with its own overview block (`## Summary`, `## Goal`, the amendment header, `**At a glance:**`) |
| spec-planner | `question` · `spec-gap` · `ready` (`count=` stages) | `artifacts/<slug>/plan.md` | `artifacts/<slug>/plan.summary.md` |
| spec-implementer | `inreview` · `committed` · `blocked` · `spec-gap` | stage card (`stage=`) | none — the card is the summary |
| spec-reviewer | `clean` · `findings` (`count=` blockers+majors, `stage=` list) · `blocked` | `artifacts/<slug>/review.md` | `artifacts/<slug>/review.verdict.md` |

`file` is lossless, for the next agent: as long as it must be. `summary` is for the user: decisions and open findings only, rewritten whole on every pass, capped at 25 lines / 2 KB (`show-file.sh` refuses a larger one and exits 3 — the agent trims it).

- `question` = one decision for the user, carried verbatim in `question=`; orchestrator relays the answer to the **same** agent (`SendMessage`), never respawns.
- An agent returning more than the status line is told to move the rest into its file and answer again.
- A status line naming a file the user must approve (`spec-diff.md`, `issue.md`, `pr.md`, the `summary=` path — `plan.summary.md`, `review.verdict.md`): orchestrator runs `sh .claude/scripts/show-file.sh <path>` before asking — opens it in a viewer outside the context and prints one line.
- Approval prompt is one line: gate, slug, the status line's counts, the answers accepted. User pulls one heading of the full file with `extract-section.sh` for the *why*.

Reviewer scope tokens: `plan` (gate 2), `stage s<n>`, `wave w<n>`, `branch` (gate 3).

### Gate 1 — spec diff. Stop for approval.

`spec-author` drafts (its own file holds the `spec-diff.md` shape and content rules), user decides, orchestrator relays. Spawn `spec-author` with: user's goal verbatim, affected area(s), `artifacts/<slug>/`.

- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning.
- `status=question` → relay question and answer without comment. `status=no-diff` (spec already covers it; the file names the violated requirement) → continue to gate 2.
- `status=ready` → show `spec-diff.md`. Change requested → relay to the same agent. Approved → record `gate1` on parent card; agent stays alive for gate 1b.

### Gate 1b — tracking issue. Stop for approval.

Orchestrator searches existing issues (`gh issue list --state all`) for the same goal and passes a candidate's number to `spec-author`, which answers `status=reuse` or `status=new`. Reuse, never duplicate.

`status=new` → same `spec-author` writes `artifacts/<slug>/issue.md` (content rules in its file). User approves → orchestrator files:

```sh
gh issue create --title "$(head -1 artifacts/<slug>/issue.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue.md)
```

Record `issue` on the parent card. **Never edit the issue body after filing** — a later spec change is `spec-author` → `issue-comment.md` → `gh issue comment --body-file`. An edited body destroys the originally-filed vs refined-later record.

### Gate 2 — implementation plan. Stop for approval.

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md` path, affected area(s), anything the user volunteered at gate 1. Nothing else; the agent explores the repo itself. It writes `plan.md` (shape: `spec-planner.md`'s `## Output`) and `plan.summary.md`.

- `status=question` → relay, resume same agent.
- `status=spec-gap` (approved text doesn't cover something the plan needs) → agent stays paused; run *Reconcile the spec*, resume the *same* planner.
- `status=ready` → fresh `spec-reviewer`, scope `plan`. `clean`, or `findings` with `count=0` → show `plan.summary.md`, then `review.verdict.md` if it lists minors, and ask. `count>0` → resume planner with `review.md` path; re-review.

The plan's dependency tree reads as waves: a stage is runnable once its dependencies merge.

Approval also settles **how** — unanswered = not approved:
- **Sequential** — one fresh `spec-implementer` runs every stage in plan order.
- **Parallel** — user gives max concurrent agents; fresh implementer per stage, separate worktrees. Waves capped at that number.

Default sequential; never infer concurrency from plan shape.

**On approval, in order:**
1. Create the worktree: `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
2. Record `gate2`+`mode` on parent card, move parent → `inprogress/`.
3. Create `open/<slug>.s<n>.md` per stage (`s0` included), `files`/`blocked-by` copied from the tree — `sh .claude/scripts/extract-section.sh '## Shared' artifacts/<slug>/plan.md` is the one plan read the orchestrator makes, for exactly those two fields. Parallel: also `open/<slug>.w<n>.md` per wave. Sequential: one wave-gate card for the run. Stage ids match plan ids.
4. Spawn `spec-author` for the **draft form** of `artifacts/<slug>/pr.md` (inputs: `issue.md`, `plan.summary.md`; content rules in its file). No approval stop — it restates already-approved text and gate 4 replaces it whole.
5. Spawn the implementer. Its first stage, `s0`, lands the approved spec text — normative only — as the branch's first commit.

### Implement, stage by stage

#### Every stage

- Not done until the plan's Verification method has run and its outcome is reported. Waiving it requires asking.

#### Sequential

One fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards. It reads its own rules (`.claude/agents/spec-implementer.md`, `AGENTS.md`) and works stages in plan order, one plan section at a time:

1. Start: stage card `open`→`inprogress`.
2. Green: card → `inreview`, `status=inreview stage=s<n>`. Orchestrator runs `gauntlet.sh` and the per-stage review below.
3. Both clean → user approval stop. Approved → same implementer (resumed) commits, answers `status=committed`.
4. Orchestrator pushes the worktree (push is never the implementer's), re-runs `gauntlet.sh` on the pushed sha, card → `done`.
5. **The first push (`s0`) opens the draft PR.** Orchestrator appends `Closes #<issue>` as the last line of `pr.md`'s draft form (the one PR-body line it writes itself), opens the PR from that file, records `pr` on the parent card, logs `pr=draft`. Every later push (step 4) updates it; the user reviews there (*PR feedback*).

```sh
gh pr create --draft --title "$(head -1 artifacts/<slug>/pr.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr.md)
```

#### Per-stage review

Sequential only; parallel's equivalent is the wave gate (parallel step 5). Every green stage, before its approval stop:
- Fresh `spec-reviewer`. Base ref = previous stage's commit (branch point for the first stage); scope = that stage id; gate 3's four axes on one stage's diff.
- `clean`, or `findings` with `count=0` → show `review.verdict.md`, forward the gauntlet line, approval stop.
- `count>0` → card → `inprogress/`, resume implementer with `review.md` path and stage id, re-run `gauntlet.sh`, fresh reviewer. An unreviewed stage is never committed.

#### Parallel waves

One worktree+branch per agent, branched off the feature branch's tip at wave start (already containing every earlier wave's merged stages, so a dependent stage's worktree never starts without its dependencies' code): `git worktree add .claude/worktrees/<slug>-<n> -b <type>/<slug>-<n> <type>/<slug>`. One implementer each; never two agents in one worktree. Each wave:

1. Runnable stage cards (`blocked-by` all in `done/`) up to approved count. Wave-gate card → `inprogress/`.
2. One implementer per stage: its worktree path, its stage only, its own card's absolute path. It commits its stage on green in its own worktree (a merge needs a commit) — unreviewed, never leaves the worktree until step 4 clears it.
3. Wait for the whole wave; each card lands in `inreview/`.
4. Per card: `gauntlet.sh` in its worktree, fresh `spec-reviewer` in `stage s<n>` scope (base = wave's branch point). `clean` → merge into feature branch — **this merge is the new base** every later wave branches from — `gauntlet.sh` on the merged result, push the feature branch (the first push opens the draft PR: sequential step 5), card → `done/`, remove worktree. `findings` → card → `inprogress/`, same implementer resumes with `review.md` path, fixes, amends its stage commit, back to this step.
5. All `done` → wave gate → `inreview/`: fresh `spec-reviewer`, scope `wave w<n>`. `clean` → wave gate `done/`, **stop: ask the user for approval before the next wave** — after *PR feedback* has run; its merge is the next wave's base, a fresh checkpoint. `findings`, red gauntlet, or merge conflict → stop, forward the status line, implicated stage cards → `inprogress/`; fresh implementer per card gets `review.md` path.

Merge conflict between two stages in a wave = dependency tree was wrong — report, fix the tree, never hand-resolve and continue. Mid-wave stop stops that wave only: finished branches still merge, the rest re-plans.

Gate 3 and the spec reconcile are unchanged under parallelism: once, on the merged feature branch, never per agent.

### PR feedback

The user reviews on the draft PR, as inline comments on the diff. Once `pr` is on the parent card, **before every approval stop** (per-stage, wave gate, gate 3, gate 4) and whenever the user says to check the PR:

```sh
bash .claude/scripts/pr-feedback.sh fetch <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md
```

One status line: `pr=<n> state=… draft=… threads=<count> file=…`.

- `threads=0` → proceed to the approval stop.
- `threads>0` → this is a review pass, handled like `review.md` findings with `count>0`: log `feedback=<count>` on the parent card, implicated stage cards → `inprogress/` (a thread's `path:` names the file; the stage whose `files` list carries it owns it — unknown → the last stage), resume the same implementer (sequential) or a fresh one per card (parallel) with the `pr-feedback.md` path. It fixes, fills each thread's `reply:` line with what changed, and answers `status=inreview`; then the usual *Per-stage review* path on the touched stages. After the push:

```sh
bash .claude/scripts/pr-feedback.sh reply <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md
```

posts every non-empty `reply:` as a thread reply and resolves that thread; threads left with an empty `reply:` stay open for the user. `status=spec-gap` from the implementer (a comment asks for behavior the approved spec doesn't cover) → *Reconcile the spec*, the thread stays open until the amended text lands. `status=blocked` (a comment is a question, or ambiguous) → relay `reason=` to the user as a question, never answer on the PR for them.

A thread the user resolves themselves, or one whose `reply:` they answer on the PR, drops out of the next fetch. Gate 4 does not run while `threads>0`.

### Reconcile the spec

Planner or implementer returns `status=spec-gap reason=…` — approved text doesn't cover what the plan needs, or behavior must differ from gate 1 approval. Normative, **reopens gate 1**:
1. `spec-author` amends `spec-diff.md` (old → new, what forced it), drafts `issue-comment.md`.
2. User approves; orchestrator posts (`gh issue comment --body-file`).
3. The *same* agent resumes — an implementer lands the amended text in `docs/specs/` before continuing.

Wrong cross-reference / clumsy wording = editorial: implementer fixes in place, no approval. Gate 3's reviewer diffs the branch's spec against `spec-diff.md` — the final spec report, never one the orchestrator composes.

### Gate 3 — review. Stop for approval.

Before proposing a PR, whole-branch pass — cross-stage bugs, spec drift across stages, scope creep:
- Fresh `spec-reviewer`, scope `branch`. Give it base ref, artifact dir, worktree path, all stage ids; it reads its own rules (`AGENTS.md`, `spec-reviewer.md`'s `## Four axes, reported separately`).
- Orchestrator runs `gauntlet.sh` on the branch, forwards both status lines, shows `review.verdict.md`.
- `count>0` → as *Per-stage review*, one fresh implementer per implicated card. Findings needing a user decision are relayed as questions, never fixed unasked.
- **Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage id, and rewrites `review.verdict.md`. Parent card → `inreview/` when review starts.

### Gate 4 — pull request. Stop for approval.

- Gauntlet + gate 3 clean, *PR feedback* at `threads=0`, then **ask whether to mark the PR ready** — user may want a manual run first; don't pre-empt it.
- `spec-author` rewrites `artifacts/<slug>/pr.md` in **full form** (content rules in its file; `.github/PULL_REQUEST_TEMPLATE.md` carries the same four sections for human PRs). User approves; orchestrator appends `Closes #<issue>` as the last line, pushes, replaces the draft's title and body, and marks it ready. Log `pr=ready` on the parent card:

```sh
gh pr edit <pr> --title "$(head -1 artifacts/<slug>/pr.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr.md)
gh pr ready <pr>
```

- No draft exists (run predates this rule, or resumed from a board that lacks `pr`): `gh pr create` with the same title and body file instead of `edit`+`ready`.
- CI fails → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`.

### Merge

Squash merge to `main`, the message carrying requirement IDs + why (`AGENTS.md` `## Spec-driven`). Then:

```sh
git worktree remove .claude/worktrees/<slug>
git worktree list   # nothing under .claude/worktrees/ should remain
```

Per-wave worktrees are removed at wave end; this sweep catches stragglers. Parent card → `done/` — no card for this run stays outside `done/`.

Merged and worktrees clean → **delete every card for this run** (stage, wave-gate, parent): `done/` was never the archive. Then ask the user for final "work done" approval — distinct from gate 4's PR approval. Approved → also delete `artifacts/<slug>/`. Declined → leave cards and artifacts; sort out the decline's cause before removing either.

### Resume an interrupted run

Cards outside `open/`+`done/`, no agent running = session died mid-run. Resume triggered by the user or `/spec-feature`, never automatic.

No worktree on the card → died during gate 1 dialog or gate 2 planning, nothing on disk to reconcile — resume the conversation from `spec-diff.md`/`plan.md`'s last state. Past gate 2 → table below. **Any resumed implementation spawns a fresh agent.**

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `pr=<n>` | `pr-feedback.sh fetch <n> …` — its status line's `state=`/`draft=` | closed/merged: run is over; missing: draft never opened, the next push opens it |
| `gauntlet=pass` | `gauntlet.sh` at that sha | card overstated state |
| stage `done` | `git branch --contains` vs feature branch | never merged; downstream plans a lie |

Report differences first. Agree → resume. Disagree → stop and report: card behind git is a forgotten move, correctable; card claiming what git can't show is never trusted into being true.

Clean reconcile → resume only no-approval work (respawn implementers for approved stages, merge finished branches, run wave gates); halt at the first gate needing the user. Recorded `gate1`/`gate2` approvals stay valid.
