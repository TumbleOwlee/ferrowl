# AGENTS.workflow.md

Gate/task-board mechanics for the orchestrator. Subagents (spec-author/planner/implementer/reviewer) never read it. Every heading below is one `extract-section.sh` pull.

### Principles

- Replaces any generic workflow skill (`/workflow`); `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`, `fix`, `docs`}. **Enforced:** `.claude/scripts/hook-guard-shell.sh` denies `git commit` on `main` and `git push` targeting `main`.
- **Orchestrator never changes spec or code and never reads spec, code, or any agent-written file.** It spawns agents, relays one question at a time between agent and user, runs git/`gh` plumbing (worktree add/remove, merge, push, `gh … --body-file`), runs `.claude/scripts/gauntlet.sh`, moves cards. Every other output is an agent's file (*Agent hand-off*).
- **Gate 1 is a dialog with the user, drafted by `spec-author`.** Existing spec + goal, nothing about current code.
- **Non-behavior change (`AGENTS.md` `## Workflow`) skips gate 1:** no `spec-diff.md`; parent card records `gate1: skipped`. Gate 2 planning finds a spec gap → stop, run gate 1, continue.
- **Every run picks a lane** — fast or full — at its first approval stop (*Lane*). The user chooses; size sets the recommended default.
- **Nothing is published before the plan is approved.** The planner is the first agent to check the spec against the code, so the tracking issue is filed at gate 2 approval, never earlier (*Tracking issue*). Sole exception: `/spec-request`, which has no plan.
- **The issue body is the current spec.** Every spec revision after filing rewrites the body and posts a short changelog comment; GitHub's edit history keeps every earlier body. A reader never replays comments to learn what is approved.
- **A spec gap that serves something other than the run's goal is split off, not folded in** (*Reconcile the spec*). Folding it in is the user's explicit override.
- Gate 2 onward delegates to agents. **All agents Sonnet or better.**
- **Issue, PR and merge are the orchestrator's alone**, from files scripts assemble (`issue-body.sh`, `pr-body.sh`, `merge-message.sh`), title = line 1 with its `# ` stripped, body = the rest, never retyped. No agent but `spec-author` learns an issue or PR number exists — spawn briefs never carry one.
- **Every text a human reads is one fact per line** — lists and sub-lists, never `;`-joined sentences; normative spec text (one entry, one line) is the sole exception. Holds for the approval prompt as much as for files.
- **The draft PR is the user's review surface** (opened by the first push, *Implement*; promoted, never replaced, at *Gate 4*). Feedback re-enters the run only through `bash .claude/scripts/pr-feedback.sh` (*PR feedback*): the file it writes goes to the implementer like `review.md`.
- **One git worktree per issue per agent**, `.claude/worktrees/<slug>` — inside project dir (agent-reachable), gitignored. Created only once gate 2 is approved (first thing to touch disk); removed after merge.
- **Plan is a contract; planner never implements.** A wrong plan stops the implementer (`spec-implementer.md` `## Stop and report — never improvise`), never gets improvised around.
- **An agent's self-reported verification is not verification.** Re-run the tools.
- **Every state change moves a task-board card.** State living only in conversation dies with the session.
- **An area whose spec costs real context to read gets a split proposed** by `spec-author` at gate 1 (rules in its file); user approves, never silent.

### Lane — fast or full

The lane sets how many approval stops the run makes. Asked once, inside the run's first approval stop — gate 1 (`ready`/`no-diff`) for a behavior change, gate 2 for a non-behavior change — as part of that same prompt, recommended lane first. Recorded as `lane` on the parent card.

Recommended default, by size:
- **fast** — `no-diff`, or ≤ 3 spec entries (gate 1 `count=`); non-behavior change with ≤ 2 stages besides `s0` (gate 2 `count=`).
- **full** — anything bigger.

| | fast | full |
|---|---|---|
| Spec diff | approved at gate 2's stop, with the plan | own stop at gate 1 |
| Gate 2 stop | spec diff + plan summary + issue draft | plan summary + issue draft, mode |
| Mode | sequential only | sequential or parallel |
| Stages | committed on green, pushed, no per-stage review or stop | per-stage review + approval stop |
| End | one stop: branch review verdict + PR body, approve = ready | gate 3 stop, then gate 4 stop |

The plan outgrowing the fast default (more than 2 stages besides `s0`) → gate 2's stop proposes switching to full; never switched silently. The user may switch lanes at any later stop.

### Verify before an approval stop

By running, never by reading:
- **Gauntlet** — `sh .claude/scripts/gauntlet.sh <worktree> artifacts/<slug>` in the agent's worktree: one status line in context, full output in `artifacts/<slug>/gauntlet.log`.
- **Every reviewer is a fresh `spec-reviewer`** — never the implementer, never a resumed reviewer: one sharing the implementer's context reproduces its blind spots.
- **Minors never re-trigger the fix-and-review loop and never wait for the user.** The verdict names where they go (`Minors (folded into s<n> | gate 4 fixes)`); the orchestrator logs each as `<ts> minor=<verdict line>` on that stage's card (parent card for gate 4 fixes) and repeats them in the implementer's next brief, and the implementer folds them into that work. A clean branch review with minors still resumes the implementer once (fast lane: commit on green), gauntlet, then gate 4. Only `count>0` (blockers + majors) loops.

### Task board

Directory a card sits in **is** its state:

```
.claude/tasks/
  open/  inprogress/  inreview/  done/   cards move between these
  artifacts/<slug>/
    spec-diff.md       gate 1 normative text, current revision
    plan.md            gate 2 stages, steps, dependency tree — the implementer's contract
    plan.summary.md    gate 2 decisions the user approves, capped
    review.md          review findings + history, keyed by stage id, append-only
    review.verdict.md  open findings per stage, rewritten every pass, capped
    gauntlet.log       full build/test/lint output of the last gauntlet.sh run
    question.md        an agent's batch of decisions for the user, rewritten per batch
    issue.md           `# <title>` (line 1) + `## Goal` — issue-body.sh assembles the body from it
    request.md         a reused issue's original body, stored once at reuse
    issue-comment.md   changelog comment of the latest spec revision after filing
    split-issue.md     `# <title>` + body of an issue split off by *Reconcile the spec*
    pr.md              `# <title>` (line 1) + gate 4 body — pr-body.sh prepends the status line and appends `Closes`
    merge-message.txt  merge-message.sh output, shown at the gate 4 stop
    pr-feedback.md     unresolved PR review threads (pr-feedback.sh fetch), reply blocks filled by the implementer
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
title: <title>
blocked-by: [<slug>.s2]
files: [src/x.rs, tests/x.rs]
branch: <type>/<slug>-3
worktree: .claude/worktrees/<slug>-3
---
2026-01-02T14:02 spawn agent=impl
2026-01-02T14:12 gauntlet=pass
```

Stage frontmatter `title` = the stage's display name, the text after `## Stage s<n>: ` in its `plan.md` heading.

Log line = `<ISO minute> <event> [key=value …]`; the implementer's own file carries the stage-event vocabulary. Every writer, orchestrator included, stamps with `date -u +%FT%H:%M` — UTC on every card, so a resumed session can order events across cards.

Parent frontmatter: `issue`, `pr` (number, set when the draft opens), `branch`, `lane: fast|full` (set at the first approval stop), `mode: sequential|parallel(N)`, `gate1` approval date, `with-plan` (fast lane, until gate 2) or `skipped`, `gate2` approval date, current `wave`, `split: [<n>, …]` (issues split off), `upstream: [<repo>#<n>, …]`, `artifacts`. `card-field.sh <slug> <field>` reads one. Never the goal or normative text — issue holds the goal, `artifacts/` the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review | full lane: reviewed + on the feature branch + orchestrator-verified; fast lane: committed + pushed + orchestrator-verified (reviewed at gate 3) |
| wave gate | not started | stages running | all done; reviewing wave diff | clean — next wave unblocks |
| parent | gate 1 or 2 pending | implementing | gate 3/4 | PR squash-merged |

Rules:
- **No agent writes its own `done`.** Implementer stops at `inreview/` — in the fast lane with its stage already committed (`status=committed`). Orchestrator verifies with `gauntlet.sh`, only then `done`.
- **Runnable** = every `blocked-by` id in `done/`. Stage `done` = on the feature branch: pushed (sequential) or merged (parallel).
- No `blocked/` directory — blocking derives from `blocked-by`.
- Card is intent, git is fact — see *Resume*.
- `done/` is not an archive — see *Merge*.

### Agent hand-off

Every agent writes its full output to a file and ends its turn with a **status line** — nothing else.

```
status=<token> [file=<path>] [summary=<path>] [stage=s<n>] [issue=<n>] [class=<class>] [reason=<one line>] [count=<n>]
```

| Agent | `status` tokens | `file` | `summary` |
|---|---|---|---|
| spec-author | `question` (`count=` decisions) · `ready` (`count=` spec entries at gate 1; `class=editorial\|normative\|split` on a revision) · `no-diff` (`count=0`) · `reuse` (`issue=`) · `new` — both with `file=` `issue.md` | `artifacts/<slug>/spec-diff.md`, `question.md`, `issue.md`, `issue-comment.md`, `split-issue.md`, `pr.md` | none — `prompt-lines.sh` quotes each file |
| spec-planner | `question` (`count=` decisions) · `spec-gap` · `ready` (`count=` stages) | `artifacts/<slug>/plan.md`, `question.md` | `artifacts/<slug>/plan.summary.md` |
| spec-implementer | `inreview` · `committed` · `blocked` · `spec-gap` | stage card (`stage=`) | none — the card is the summary |
| spec-reviewer | `clean` · `findings` (`count=` blockers+majors, `stage=` list) · `blocked` | `artifacts/<slug>/review.md` | `artifacts/<slug>/review.verdict.md` |

`file` is lossless, for the next agent: as long as it must be. `summary` is for the user: decisions and open findings only, rewritten whole on every pass, capped at 25 lines / 2 KB (`show-file.sh` refuses a larger one and exits 3 — the agent trims it).

- `question` = a batch of decisions in `artifacts/<slug>/question.md` (*Relayed question*); orchestrator relays the answers to the **same** agent (`SendMessage`), never respawns.
- An agent returning more than the status line is told to move the rest into its file and answer again.
- A status line naming a file the user must approve (`spec-diff.md`, `issue.md`, `split-issue.md`, `pr.md`, `merge-message.txt`, the `summary=` path — `plan.summary.md`, `review.verdict.md`): orchestrator runs `sh .claude/scripts/show-file.sh <path> [<path> ...]` before asking — every file of one approval stop in a single call; it opens each outside the context and prints, per file, one line naming how it was shown plus that file's quotable prompt lines (`prompt-lines.sh`) — the only file content the orchestrator ever holds.
- The approval prompt is built from those lines alone (*Approval prompt*). User pulls one heading of the full file with `extract-section.sh` for the *why*.

Reviewer scope tokens: `plan` (gate 2), `stage s<n>`, `wave w<n>`, `branch` (gate 3).

### Approval prompt

Every approval stop is one message in this fixed shape, composed from `show-file.sh`'s quotable lines, the agents' status-line tokens and the fixed lists below — the orchestrator writes no line of its own about content. A block absent from this stop is omitted; order is fixed.

```
Gate <n> · <slug> · <files shown, with rev> [· fast lane]
Goal: <spec-diff.md's Goal line>
Entries: <spec-diff.md's counts and IDs>
Plan: <plan.summary.md's Decide line>
Issue: <new | reuse #<n>> — <issue.md's title>                                        ← new/reuse from the author's status line
Split: <split-issue.md's title>
Verification:
- <plan.summary.md's lines>
Confirm:
- <prompt-lines.sh's single Confirm block over the shown files>                       ← or `Confirm: nothing flagged`
Review: <verdict line>
Spec: <n of m pinned …>
Minors (folded into <where>):
- <verdict's minor lines>                                                            ← information, not a decision
Needs user:
- <verdict's question and options>
Approving will:
- <the fixed list for this gate and lane, below>
Answer: <the fixed answer line for this gate>
```

Fixed `Approving will:` / `Answer:` per stop:

| Stop | Approving will | Answer |
|---|---|---|
| gate 1, full | settle the lane · hand the spec to the planner (gate 2) | `approve [full\|fast] · change <what> · stop` |
| gate 1, fast | record the lane · hand the spec to the planner; the spec is approved with the plan | `approve · switch to full · change <what> · stop` |
| gate 2 | file the issue (or rewrite #<n>) · create worktree `<path>` on branch `<branch>` · create <n> stage cards · spawn the implementer at <first stage> (fast: commits and pushes on green, no per-stage stop) · settle the mode (full) · next stop: <per-stage s<n> \| gate 4> | `approve [sequential\|parallel N] · switch lane · change <what> · stop` |
| stage / wave | commit s<n> · push, open or update the draft PR · spawn the next stage | `approve · change <what> · stop` |
| gate 3 | hand the branch to `spec-author` for the PR body (gate 4) | `approve · change <what> · stop` |
| gate 4 | push · replace the draft's title and body · mark ready · wait for CI · squash-merge with the shown message · remove the worktree · delete the cards | `approve · manual run first · change <what> · stop` |
| work done | delete `artifacts/<slug>/` | `approve · keep` |

A `Needs user` item is answered in the same reply; the orchestrator relays it to the implementer with the approval.

### Relayed question

An agent's `status=question` names `artifacts/<slug>/question.md` (shape: `spec-author.md` `## Questions`): every decision it can state without knowing another's answer, numbered, each with `Why you:`, options with the recommendation marked, costs when options differ. Orchestrator runs `show-file.sh` on it (its prompt lines are the whole file) and asks once:

```
Question · <slug> · <gate or stage> · <agent> · <n> decisions
<the file, verbatim>
Answer: <1: a · 2: b · …> · other <text>
```

Answers go back to the same agent in one message, numbered. A decision that depended on an answer arrives in the next `question.md`. A question without a real `Why you:` is sent back: the agent decides it and records `(chosen)`.

### Gate 1 — spec diff. Stop for approval.

`spec-author` drafts (its own file holds the `spec-diff.md` shape and content rules), user decides, orchestrator relays. Spawn `spec-author` with: user's goal verbatim, affected area(s), `artifacts/<slug>/`, and the issue number when the user named one ("take issue #n") — the *Tracking issue* reuse candidate.

- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning.
- `status=question` → relay question and answer without comment.
- `status=ready` or `status=no-diff` (spec already covers the goal; the file names the violated requirements) → show `spec-diff.md`, ask approval **and lane** (*Lane*) in one prompt. Change requested → relay to the same agent.
  - **full** → approved: record `gate1` and `lane: full` on the parent card.
  - **fast** → no separate approval: record `gate1: with-plan` and `lane: fast`; the spec diff is approved at gate 2's stop.
- The agent stays alive for *Tracking issue* and any *Reconcile the spec*.

### Gate 2 — plan and tracking issue. Stop for approval.

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md` path (none for a non-behavior change), affected area(s), anything the user volunteered at gate 1. Nothing else; the agent explores the repo itself. It writes `plan.md` (shape: `spec-planner.md`'s `## Output`) and `plan.summary.md`.

- `status=question` → relay, resume same agent.
- `status=spec-gap` (approved text doesn't cover something the plan needs) → agent stays paused; run *Reconcile the spec*, resume the *same* planner.
- `status=ready` → fresh `spec-reviewer`, scope `plan`. `count>0` → resume planner with `review.md` path; re-review.
- `clean`, or `findings` with `count=0` → `spec-author` drafts `issue.md` (*Tracking issue*; no stop of its own; a non-behavior run spawns one now with the goal verbatim, `plan.summary.md`, `artifacts/<slug>/`), then **one** `show-file.sh` call: `plan.summary.md`, `review.verdict.md` if it lists minors, `issue.md`, and — fast lane — `spec-diff.md`. One *Approval prompt*: approve, plus mode (full lane), plus lane for a non-behavior change.

The plan's dependency tree reads as waves: a stage is runnable once its dependencies merge.

Mode, full lane only — unanswered = not approved:
- **Sequential** — one fresh `spec-implementer` runs every stage in plan order.
- **Parallel** — user gives max concurrent agents; fresh implementer per stage, separate worktrees. Waves capped at that number.

Default sequential; never infer concurrency from plan shape. Fast lane is always sequential.

**On approval, in order:**
1. File or rewrite the issue (*Tracking issue*); record `issue` on the parent card. Fast lane: record `gate1` too.
2. Create the worktree: `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
3. Record `gate2`+`mode` (and `lane`, for a non-behavior change) on parent card, move parent → `inprogress/`.
4. Create `open/<slug>.s<n>.md` per plan stage (`s0` only when the plan has one), `title`/`files`/`blocked-by` copied from the tree — `sh .claude/scripts/extract-section.sh '## Shared' artifacts/<slug>/plan.md` is the one plan read the orchestrator makes, for exactly those three fields. Parallel: also `open/<slug>.w<n>.md` per wave. Sequential: one wave-gate card for the run. Stage ids match plan ids.
5. Spawn the implementer, its brief naming the lane. A plan with `s0` lands the approved spec text — normative only — as the branch's first commit; a `no-diff` or non-behavior plan has no `s0`.

### Tracking issue

Drafted inside gate 2 once the plan review is clean (the spec is stable by then), approved at gate 2's stop, filed on approval. `/spec-request` runs this section right after gate 1 approval instead.

Orchestrator searches existing issues (`gh issue list --state all --search …`) for the same goal and passes candidate numbers — plus one the user named — to `spec-author`, which writes `artifacts/<slug>/issue.md` (title + goal; shape: its file) and answers `status=reuse issue=<n> file=…` or `status=new file=…` in one turn. Reuse, never duplicate. On reuse the orchestrator stores the issue's original body once with `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md` — the body as filed, or, when that body is one this workflow assembled (it carries `## Spec (rev`), the text inside its `<details>` block.

The body is assembled, never written by hand: `sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md` (status line from the parent card, goal, the spec as a list with its decisions, plan decisions, the collapsed original request). Filing:

```sh
sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md
# new
gh issue create --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
# reuse
gh issue edit <n> --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
```

**Every spec revision after filing rewrites the body**, whatever its class (*Reconcile the spec*): `spec-author` revises `spec-diff.md` and writes `issue-comment.md`, the revision's changelog entry; the orchestrator reassembles and posts, no further approval:

```sh
sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md
gh issue edit <n> --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
gh issue comment <n> --body-file artifacts/<slug>/issue-comment.md
```

The body is also reassembled and edited when the status line changes (`pr` set, a split filed). A revision before filing posts nothing. Every resume of `spec-author` for a revision says whether the issue is **filed** (changelog comment wanted) or **unfiled** (no comment) — including a change requested at gate 2's stop.

### Implement, stage by stage

#### Every stage

- Not done until the plan's Verification method has run and its outcome is reported. Waiving it requires asking.
- **The draft PR body is the board, rendered**: `sh .claude/scripts/pr-body.sh <slug> draft > artifacts/<slug>/pr-draft.md` (status line, one-line why, one checklist line per stage card with its state or sha, where to review) — regenerated and pushed with `gh pr edit <pr> --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)` on **every card move** once the PR exists. Nobody writes PR-body text by hand.
- **The first push opens the draft PR** from that file; orchestrator records `pr` on the parent card, logs `pr=draft`, reassembles the issue body (*Tracking issue*). Every later push updates it; the user reviews there (*PR feedback*).

```sh
sh .claude/scripts/pr-body.sh <slug> draft > artifacts/<slug>/pr-draft.md
gh pr create --draft --title "$(head -1 artifacts/<slug>/pr-draft.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)
```

#### Fast lane

One fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards, `lane=fast`. Per stage, in plan order:

1. Start: card `open`→`inprogress`. Green: the implementer commits, card → `inreview/`, answers `status=committed stage=s<n>` — no per-stage review, no approval stop.
2. Orchestrator runs `gauntlet.sh`, pushes (the first push opens the draft PR), logs `gauntlet=pass sha=<sha>` on the parent card, card → `done`, resumes the implementer for the next stage. Red gauntlet → card → `inprogress/`, resume the implementer with the gauntlet line.
3. Last stage `done` → *PR feedback*, then *Gate 3*: its branch review is the only review of a fast-lane run.

Fixes in the fast lane (gate 3 findings, PR feedback, red gauntlet) are committed on green too: the implementer answers `status=committed`, the orchestrator runs `gauntlet.sh` and pushes before any re-review — a review diffs commits, never a dirty worktree.

#### Sequential

Full lane. One fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards, `lane=full`. It reads its own rules (`.claude/agents/spec-implementer.md`, `AGENTS.md`) and works stages in plan order, one plan section at a time:

1. Start: stage card `open`→`inprogress`.
2. Green: card → `inreview`, `status=inreview stage=s<n>`. Orchestrator runs `gauntlet.sh` and the per-stage review below.
3. Both clean → user approval stop. Approved → same implementer (resumed) commits, answers `status=committed`.
4. Orchestrator pushes the worktree (push is never the implementer's), re-runs `gauntlet.sh` on the pushed sha, logs `gauntlet=pass sha=<sha>` on the parent card, card → `done`.

#### Per-stage review

Full lane, sequential only; parallel's equivalent is the wave gate (parallel step 5). Every green stage, before its approval stop:
- Fresh `spec-reviewer`. Base ref = previous stage's commit (branch point for the first stage); scope = that stage id; gate 3's four axes on one stage's diff.
- `clean`, or `findings` with `count=0` → show `review.verdict.md`, forward the gauntlet line, approval stop; its `Minors` go onto the next stage's card as `minor=` log lines.
- `count>0` → card → `inprogress/`, resume implementer with `review.md` path and stage id, re-run `gauntlet.sh`, fresh reviewer. An unreviewed stage is never committed.
- **One code stage** (besides `s0`): review in scope `branch` instead — it is gate 3's review, and the stage's approval stop is gate 3's stop. Gate 3 re-runs only if code changed after it.

#### Parallel waves

Full lane only. One worktree+branch per agent, branched off the feature branch's tip at wave start (already containing every earlier wave's merged stages, so a dependent stage's worktree never starts without its dependencies' code): `git worktree add .claude/worktrees/<slug>-<n> -b <type>/<slug>-<n> <type>/<slug>`. One implementer each; never two agents in one worktree. Each wave:

1. Runnable stage cards (`blocked-by` all in `done/`) up to approved count. Wave-gate card → `inprogress/`.
2. One implementer per stage: its worktree path, its stage only, its own card's absolute path. It commits its stage on green in its own worktree (a merge needs a commit) — unreviewed, never leaves the worktree until step 4 clears it.
3. Wait for the whole wave; each card lands in `inreview/`.
4. Per card: `gauntlet.sh` in its worktree, fresh `spec-reviewer` in `stage s<n>` scope (base = wave's branch point). `clean` → merge into feature branch — **this merge is the new base** every later wave branches from — `gauntlet.sh` on the merged result, push the feature branch (the first push opens the draft PR, *Every stage*), card → `done/`, remove worktree. `findings` → card → `inprogress/`, same implementer resumes with `review.md` path, fixes, amends its stage commit, back to this step.
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
- `threads>0` → this is a review pass, handled like `review.md` findings with `count>0`: log `feedback=<count>` on the parent card, implicated stage cards → `inprogress/` (a thread's `path:` names the file; the stage whose `files` list carries it owns it — unknown → the last stage), resume the same implementer (sequential, fast) or a fresh one per card (parallel) with the `pr-feedback.md` path. It fixes, fills each thread's `reply:` block, and answers `status=inreview`; then the usual review path on the touched stages (fast lane: gate 3's branch review). After the push:

```sh
bash .claude/scripts/pr-feedback.sh reply <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md s<n> <sha>
```

posts every filled reply block: `reply: fixed` as `Fixed in s<n> (<sha>)` plus its bullets, resolving the thread (`<stage> <sha>` arguments: the stage and the pushed head); `reply: declined` as `Not changed — <reason>` plus its bullets, leaving the thread open for the user; an empty `reply:` is untouched. `status=spec-gap` from the implementer (a comment asks for behavior the approved spec doesn't cover) → *Reconcile the spec*, the thread stays open until the amended text lands. `status=blocked` (a comment is a question, or ambiguous) → relay `reason=` to the user as a question, never answer on the PR for them.

A thread the user resolves themselves, or one whose `reply:` they answer on the PR, drops out of the next fetch. Gate 4 does not run while `threads>0`.

### Reconcile the spec

Planner or implementer returns `status=spec-gap reason=…` — the spec text doesn't cover what the plan needs, or behavior must differ from it. The paused agent stays paused. Orchestrator resumes `spec-author` with the reason verbatim; it revises `spec-diff.md` (rev N+1) and answers `status=ready class=<class> file=…`, after a `question` when the class is `beyond`. The author classifies (rules in its file):

| `class` | What | Stop |
|---|---|---|
| `editorial` | no normative meaning changes: wording, a cross-reference, a violated ID added to or dropped from a `no-diff`, an entry moved | none — orchestrator logs `spec-rev=<N> class=editorial` and continues |
| `normative` | an entry added, changed or dropped that the run's goal needs | approval stop on `spec-diff.md` — except a fast-lane run before gate 2's stop, which shows the revision there |
| `beyond` | needed only because of something outside the goal: an area the gate 1 Summary didn't name, an upstream change, or more than 5 entries | the author's `question` first: **split** (recommended) or fold in. Its answer then comes back as `class=split` or `class=normative` |

- **Split** (`class=split`) — the author narrows the triggering entry to what holds today, or states the gap as a deliberate edge case, and drafts `split-issue.md` for the rest (goal, what was found, existing IDs only). Approval stop on `spec-diff.md` + `split-issue.md` — a fast-lane run before gate 2's stop shows both there instead. Approved → orchestrator files `split-issue.md` (`gh issue create`, as in *Tracking issue*) with `Split from #<issue>` appended as its last line — at once if the tracking issue is filed, else right after gate 2 approval step 1 — appends it to the parent card's `split:` and reassembles the issue body. The run continues on the narrowed spec; the new issue is its own later run. The split issue's body is `split-issue.md` with the status line `Split from #<issue> (rev <N>) · no spec yet` prepended.
- **Fold in** — the user's explicit override: handled as `normative`, and the author rewrites the goal line to state the widened scope.

Then, in order:
1. Issue already filed → body rewrite + changelog comment (*Tracking issue*). Not filed yet → nothing to post.
2. The *same* paused agent resumes. An implementer revision after `s0` (or in a plan without one) lands in `docs/specs/` before it continues, inside its current stage's commit; the revision's `docs/specs/` files join that stage's `files` (orchestrator appends them to the card and logs `files+=…`). Parallel: never inside a wave stage — a fresh implementer lands it as its own commit on the feature branch before the next wave branches.

Gate 3's reviewer diffs the branch's spec against `spec-diff.md` — the final spec report, never one the orchestrator composes.

### Gate 3 — review. Stop for approval.

Before proposing a PR, whole-branch pass — cross-stage bugs, spec drift across stages, scope creep:
- Fresh `spec-reviewer`, scope `branch`. Give it base ref, artifact dir, worktree path, all stage ids; it reads its own rules (`AGENTS.md`, `spec-reviewer.md`'s `## Four axes, reported separately`). Already run, with its stop, as a one-code-stage run's stage review (*Per-stage review*) → skip to gate 4 unless code changed since.
- Orchestrator runs `gauntlet.sh` on the branch, forwards both status lines, shows `review.verdict.md`.
- `count>0` → as *Per-stage review*, the same implementer (fast lane, sequential) or one fresh implementer per implicated card (parallel), then a fresh branch review. Findings needing a user decision are relayed as questions, never fixed unasked.
- Log `review=clean` or `review=<n>-resolved` on the parent card once the branch review is clean (the PR status line reads it). Minors from it go onto the parent card as `minor=` log lines; the orchestrator resumes the implementer with them before `spec-author` writes `pr.md` (fast lane: committed on green), re-runs the gauntlet, and logs `gauntlet=pass sha=<sha>` on the parent card.
- **Fast lane:** no stop here — clean goes straight to gate 4, whose stop shows the verdict.
- **Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage id, and rewrites `review.verdict.md`. Parent card → `inreview/` when review starts.

### Gate 4 — pull request. Stop for approval.

- Gauntlet + gate 3 clean, *PR feedback* at `threads=0`.
- `spec-author` writes `artifacts/<slug>/pr.md` in **full form** (content rules in its file; `.github/PULL_REQUEST_TEMPLATE.md` carries the same sections for human PRs). Orchestrator assembles both outputs:

```sh
sh .claude/scripts/pr-body.sh <slug> full > artifacts/<slug>/pr-full.md
sh .claude/scripts/merge-message.sh <slug> > artifacts/<slug>/merge-message.txt
```

- **One stop**: `show-file.sh` on `pr.md`, `merge-message.txt` (fast lane: plus `review.verdict.md`); the *Approval prompt* offers **approve** or **manual run first** — the user may want one; don't pre-empt it. Approved → push, replace the draft's title and body, mark ready, log `pr=ready`, wait for CI, then *Merge*:

```sh
gh pr edit <pr> --title "$(head -1 artifacts/<slug>/pr-full.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-full.md)
gh pr ready <pr>
```

- No draft exists (run predates this rule, or resumed from a board that lacks `pr`): `gh pr create` with the same title and body file instead of `edit`+`ready`.
- CI fails → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`; the fix is a stage card back to `inprogress/`, then gate 4 again.

### Merge

The orchestrator merges, after gate 4 approval and green CI, with the message approved there — never the GitHub default:

```sh
gh pr merge <pr> --squash --subject "$(head -1 artifacts/<slug>/merge-message.txt)" --body-file <(tail -n +3 artifacts/<slug>/merge-message.txt)
```

Then:

```sh
git worktree remove .claude/worktrees/<slug>
git worktree list   # nothing under .claude/worktrees/ should remain
```

Per-wave worktrees are removed at wave end; this sweep catches stragglers. Parent card → `done/` — no card for this run stays outside `done/`.

Merged and worktrees clean → **delete every card for this run** (stage, wave-gate, parent): `done/` was never the archive. Then ask the user for final "work done" approval — distinct from gate 4's PR approval. Approved → also delete `artifacts/<slug>/`. Declined → leave cards and artifacts; sort out the decline's cause before removing either.

### Resume an interrupted run

Cards outside `open/`+`done/`, no agent running = session died mid-run. Resume triggered by the user or `/spec-feature`, never automatic.

No worktree on the card → died during gate 1 dialog or gate 2 planning, nothing on disk to reconcile — resume the conversation from `spec-diff.md`/`plan.md`'s last state. `issue` set but no worktree → gate 2 was approved: continue at its step 2, never re-file. Past gate 2 → table below. **Any resumed implementation spawns a fresh agent**, its brief naming the card's `lane`.

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `issue=<n>` | `issue-view.sh <n>` — body's `## Spec (rev N)` vs `spec-diff.md`'s rev | body behind: a revision never posted, rewrite it (*Tracking issue*) |
| `pr=<n>` | `pr-feedback.sh fetch <n> …` — its status line's `state=`/`draft=` | closed/merged: run is over; missing: draft never opened, the next push opens it |
| `gauntlet=pass` | `gauntlet.sh` at that sha | card overstated state |
| stage `done` | `git branch --contains` vs feature branch | never merged; downstream plans a lie |
| PR `## Stages` | the PR body vs `pr-body.sh <slug> draft` | body behind the board: a move never rendered, `gh pr edit` with the fresh render |

Report differences first. Agree → resume. Disagree → stop and report: card behind git is a forgotten move, correctable; card claiming what git can't show is never trusted into being true.

Clean reconcile → resume only no-approval work (respawn implementers for approved stages, merge finished branches, run wave gates); halt at the first gate needing the user. Recorded `gate1`/`gate2` approvals stay valid.
