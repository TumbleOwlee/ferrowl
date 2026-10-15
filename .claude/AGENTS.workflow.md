# AGENTS.workflow.md

Gate/task-board mechanics for the orchestrator. Subagents (spec-author/architect/planner/implementer/reviewer) never read it.

### Principles

Every run, `/spec-request` included.

- Replaces any generic workflow skill (`/workflow`); `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`, `fix`, `docs`}. **Enforced:** `.claude/scripts/hook-guard-shell.sh` denies `git commit` on `main` and `git push` targeting `main`.
- **Orchestrator never changes spec or code and never reads spec, code, or any agent-written file.** It spawns agents, relays one question at a time between agent and user, runs git/`gh` plumbing (worktree add/remove, merge, push, `gh … --body-file`), runs `.claude/scripts/gauntlet.sh`, moves cards. Every other output is an agent's file (*Agent hand-off*).
- **Gate 1 is a best-effort grilling, drafted by `spec-author`.** Existing spec + the request's user stories, nothing about current code. It settles scope, scenarios and the edge cases the user cares about; every other point is deferred to the agent that settles it, never anticipated.
- **Architecture is its own step, drafted by `spec-architect`**, between gate 1 and gate 2: the structure the plan must reach, checked and shaped by the user (*Architecture*).
- **The spec settles over the run** (*Spec changes*): local IDs (`S<n>`, `R<n>`, `E<n>`) until gate 2 fixes them; the architect and the planner edit `spec-diff.md` in place; after `s0` the implementer edits the branch's `docs/specs/` and a reviewer puts every meaningful change to the user.
- **Every run picks a lane** at its first approval stop (*Lane — fast or full*).
- **The issue body is the spec as filed, then as merged** (*Tracking issue*); between the two, comments carry the changes the user approved.
- **Issue and PR are the orchestrator's alone**, from files scripts assemble (`issue-body.sh`, `pr-body.sh`), title = line 1 with its `# ` stripped, body = the rest, never retyped. No agent but `spec-author` learns an issue or PR number exists — spawn briefs never carry one.
- **An agent's self-reported verification is not verification.** Re-run the tools.
- **A split of an area whose spec costs real context is `spec-author`'s to propose** at gate 1 (rules in its file) and the user's to approve, never silent.

### Principles — implementation

Gate 2 onward.

- **Non-behavior change (`AGENTS.md` `## Workflow`) skips gate 1** and starts at *Architecture*: no `spec-diff.md`; parent card records `gate1: skipped`. Architecture or planning finds a behavior change (`status=behavior-change`) → stop, run gate 1 with the reason, resume the same agent with the `spec-diff.md` path.
- **Nothing is published before the plan is approved** (*Tracking issue*).
- **A spec change that serves something other than the run's goal is split off, not folded in** (*Spec changes*).
- Gate 2 onward delegates to agents. **All agents Sonnet or better.**
- **The draft PR is the user's review surface** (*Every stage*; promoted at *Gate 4 — pull request*). Feedback re-enters the run only through *PR feedback*.
- **One git worktree per issue per agent**, `.claude/worktrees/<slug>` — inside project dir (agent-reachable), gitignored. Created only once gate 2 is approved (first thing to touch disk); removed after merge.
- **Plan is a contract; planner never implements.** A wrong plan stops the implementer (`spec-implementer.md` `## Stop and report — never improvise`).
- **Every state change moves a task-board card.** State living only in conversation dies with the session.

### Lane — fast or full

The lane sets how many approval stops the run makes. Proposed by size when the first agent output of the run is ready; the user settles it at the run's first approval stop, recommended lane first.

Recommended default, by size:
- **fast** — `no-diff`, or ≤ 3 `R`/`E` entries in the brief (gate 1 `count=`); non-behavior change: `no-arch`, or ≤ 3 new or changed components (architecture `count=`).
- **full** — anything bigger.

| | fast | full |
|---|---|---|
| Brief (gate 1) | no stop of its own — approved at the architecture stop | own stop, settles the lane |
| Architecture | one stop: brief + architecture + lane; approving pre-authorizes gate 2's *On approval* steps | own stop |
| Gate 2 | no stop unless the plan review leaves a `Needs user` item or the plan outgrows the fast default; else a notice | plan summary + issue draft, mode |
| Mode | sequential only | sequential or parallel |
| Stages | *Fast lane* | *Sequential* or *Parallel waves*, stops per *Stage stop* |
| End | one stop, *Gate 4 — pull request* | *Gate 3 — review* stop, then *Gate 4 — pull request* stop |

Recommended full, user picks fast at the brief stop: the architecture stop is fast's (without the brief). Fast with `no-arch`: brief and lane are approved at gate 2's stop, which then always stops. The plan outgrowing the fast default (more than 2 stages besides `s0`) → gate 2 stops and proposes switching to full; never switched silently. The user may switch lanes at any later stop.

### Verify before an approval stop

By running, never by reading:
- **Gauntlet** — `sh .claude/scripts/gauntlet.sh <worktree> artifacts/<slug>` in the agent's worktree: one status line in context, full output in `artifacts/<slug>/gauntlet.log`.
- **Every reviewer is a fresh `spec-reviewer`** — never the implementer, never a resumed reviewer: one sharing the implementer's context reproduces its blind spots.
- **Minors never wait for the user.** The verdict names where they go: `Minors (folded into s<n+1>)` when a stage follows, `Minors (fixed before commit)` when none does — a last stage, a branch review. Folded: the orchestrator logs each as `<ts> minor=<verdict line>` on the next stage's card and repeats them in the implementer's brief. Fixed before commit: the implementer is resumed with them at once, fixes, gauntlet, no re-review, then the stage's commit (fast lane: commit on green). Only `count>0` (blockers + majors) loops, and at most **two** fix-and-review rounds per scope: findings still open after the second round go to the user as `Needs user` items on the verdict.

### Task board

Directory a card sits in **is** its state:

```
.claude/tasks/
  open/  inprogress/  inreview/  done/   cards move between these
  artifacts/<slug>/
    spec-diff.md       gate 1 brief, then the run's spec until s0 — local IDs until gate 2; synced from the branch at gate 4
    architecture.md    components, structure, interactions, public surface, code map — the plan's target
    architecture.summary.md  architecture decisions the user approves, capped
    plan.md            gate 2 stages, steps, dependency tree — the implementer's contract
    plan.summary.md    gate 2 decisions the user approves, capped
    review.md          review findings + history, keyed by stage id, append-only
    review.verdict.md  open findings per stage, rewritten every pass, capped
    gauntlet.log       full build/test/lint output of the last gauntlet.sh run
    question.md        an agent's batch of decisions for the user (*Relayed question*)
    issue.md           title + goal (*Tracking issue*)
    request.md         the request issue's original body, stored once (gate 1 or reuse)
    issue-comment.md   changelog comment of the latest user-approved spec change after filing
    split-issue.md     an issue split off (*Spec changes*)
    pr.md              gate 4 PR title + body (*Gate 4 — pull request*)
    pr-feedback.md     unresolved PR review threads (*PR feedback*)
```

Directories tracked; cards gitignored. Cards live in **main checkout only** — a worktree agent gets its own card's id and the board root, locates the card with `ls .claude/tasks/*/<id>.md` before every write (its directory changes under the agent), writes only that file. No per-worktree board copy.

| Card | File | Owner |
|---|---|---|
| parent | `<slug>.md` | orchestrator — one per run |
| stage | `<slug>.s<n>.md` | the implementer working that stage |
| wave gate | `<slug>.w<n>.md` | orchestrator — one per parallel wave |

Cards are agent-only: YAML frontmatter + a log of terse field=value tokens, no prose. Agents **append**, never rewrite a line — crash mid-write costs one truncated line, not the file.

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

Parent frontmatter: `issue`, `pr` (number, set when the draft opens), `branch`, `lane: fast|full` (set at the first approval stop), `mode: sequential|parallel(N)`, `gate1` approval date, `with-arch` (fast lane, until the architecture stop) or `skipped`, `arch` approval date or `no-arch`, `gate2` approval date, current `wave`, `split: [<n>, …]` (issues split off), `upstream: [<repo>#<n>, …]`, `artifacts`. `card-field.sh <slug> <field>` reads one. Never the goal or normative text — issue holds the goal, `artifacts/` the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review (fast lane: already committed) | on the feature branch — pushed (sequential) or merged (parallel) — and orchestrator-verified; full lane also reviewed and approved or auto-continued |
| wave gate | not started | stages running | all done; reviewing wave diff | clean — next wave unblocks |
| parent | gate 1, architecture or gate 2 pending | implementing | gate 3/4 | PR squash-merged |

Rules:
- **No agent writes its own `done`** — the orchestrator's move (table above).
- **Runnable** = every `blocked-by` id in `done/`.
- No `blocked/` directory — blocking derives from `blocked-by`.
- Card is intent, git is fact — see *Resume an interrupted run*. Cards end at *Gate 4 — pull request*.

### Agent hand-off

Every agent writes its full output to a file and ends its turn with a **status line** — nothing else.

```
status=<token> [file=<path>] [summary=<path>] [stage=s<n>] [issue=<n>] [reason=<one line>] [count=<n>] [delta=<n>] [user=<n>] [spec=<n>]
```

| Agent | `status` tokens | `file` | `summary` |
|---|---|---|---|
| spec-author | `question` (`count=` decisions) · `ready` (`count=` `R`+`E` entries at gate 1) · `no-diff` (`count=0`) · `reuse` (`issue=`) · `new` — both with `file=` `issue.md` | `artifacts/<slug>/spec-diff.md`, `question.md`, `issue.md`, `issue-comment.md`, `split-issue.md`, `pr.md` | none — `prompt-lines.sh` quotes each file |
| spec-architect | `question` (`count=` decisions) · `ready` (`count=` new + changed components) · `no-arch` · `split` (`reason=`) · `behavior-change` (`reason=`, non-behavior run) | `artifacts/<slug>/architecture.md`, `question.md`; edits `spec-diff.md` | `artifacts/<slug>/architecture.summary.md` |
| spec-planner | `question` (`count=` decisions) · `arch-gap` (`reason=`) · `split` (`reason=`) · `behavior-change` (`reason=`, non-behavior run) · `ready` (`count=` stages, `spec=` `spec-diff.md` revs it wrote) | `artifacts/<slug>/plan.md`, `question.md`; edits `spec-diff.md` | `artifacts/<slug>/plan.summary.md` |
| spec-implementer | `inreview` · `committed` (both `delta=` spec changes) · `blocked` | stage card (`stage=`) | none — the card is the summary |
| spec-reviewer | `clean` · `findings` (`count=` blockers+majors, `user=` `Needs user` items, `stage=` list) · `blocked` | `artifacts/<slug>/review.md` | `artifacts/<slug>/review.verdict.md` |

`file` is lossless, for the next agent: as long as it must be. `summary` is for the user: decisions and open findings only, rewritten whole on every pass, capped at 25 lines / 2 KB (`show-file.sh` refuses a larger one and exits 3 — the agent trims it).

- `question` = a batch of decisions in `artifacts/<slug>/question.md` (*Relayed question*).
- An agent returning more than the status line is told to move the rest into its file and answer again.
- Every approval of `spec-diff.md` logs `spec-rev=<N>` on the parent card (the rev `show-file.sh` named). A later stop showing it again runs `show-file.sh` with `PROMPT_SINCE_REV=<that N>`, so `Confirm:` carries only the `(chosen)` lines written since.
- A status line naming a file the user must approve (`spec-diff.md`, `pr.md`, the `summary=` path — `architecture.summary.md`, `plan.summary.md`, `review.verdict.md` when it has a `Needs user` item): orchestrator runs `sh .claude/scripts/show-file.sh <path> [<path> ...]` before asking — every file of one approval stop in a single call; it opens each outside the context and prints, per file, one line naming how it was shown plus that file's quotable prompt lines (`prompt-lines.sh`), for the *Approval prompt*.

Reviewer scope tokens: `plan` (gate 2), `stage s<n>`, `delta s<n>` (one stage's spec changes, *Spec changes*), `wave w<n>`, `branch` (gate 3).

### Approval prompt

Every approval stop is one message in this fixed shape, composed from `show-file.sh`'s quotable lines, the agents' status-line tokens and the fixed lists below. A block absent from this stop is omitted; order is fixed. The user pulls one heading of the full file with `extract-section.sh` for the *why*.

```
<Gate <n> | Architecture | Delta s<n>> · <slug> · <files shown, with rev> [· fast lane]
Goal: <spec-diff.md's Goal line>
Scenarios: <spec-diff.md's count and S-IDs>
Entries: <spec-diff.md's counts and IDs>
Deferred: <spec-diff.md's count per owner>
Architecture: <architecture.summary.md's Decide line>
Surface:
- <architecture.summary.md's public-surface lines>
Plan: <plan.summary.md's Decide line>
Spec since arch: <plan.summary.md's Spec-since-architecture line>
Stops:
- <stages the plan marks stop, with why>
Issue: <new | reuse #<n>> — <issue.md's title>                                        ← new/reuse from the author's status line
Split: <split-issue.md's title>
Verification:
- <plan.summary.md's lines>
Confirm:
- <prompt-lines.sh's single Confirm block over the shown files>                       ← or `Confirm: nothing flagged`
Review: <verdict line>
Spec: <n of m pinned …>
Spec changes: <verdict's detail and need-you IDs>
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
| gate 1 (brief) | settle the lane · hand the brief to the architect | `approve [full\|fast] · change <what> · stop` |
| architecture, full | hand brief and architecture to the planner (gate 2) | `approve · change <what> · stop`; first stop of a non-behavior run: `approve [full\|fast] · change <what> · stop` |
| architecture, fast | approve the brief (when not yet approved) · record the lane · hand both to the planner · once the plan review is clean, with no further stop: file the issue (or rewrite #<n>) · create worktree `<path>` on branch `<branch>` · create the stage cards · spawn the implementer at `s0` (commits and pushes on green) · next stop: gate 4 | `approve · switch to full · change <what> · stop` |
| gate 2 | file the issue (or rewrite #<n>) · create worktree `<path>` on branch `<branch>` · create <n> stage cards · spawn the implementer at <first stage> (fast: commits and pushes on green, no per-stage stop) · settle the mode (full) · next stop: <per-stage s<n> \| gate 4> | `approve [sequential\|parallel N] [stop: s1,s3] · switch lane · change <what> · stop` |
| delta, fast lane | apply the answers (keep · revert · split) · continue at the next stage | `<1: keep \| revert \| split · …> · stop` |
| stage / wave (heading carries the *Stage stop* reason, e.g. `· stop: demo-TUI verification`) | commit s<n> · push, open or update the draft PR · spawn the next stage | `approve · change <what> · stop` |
| gate 3 | hand the branch to `spec-author` for the PR body (gate 4) | `approve · change <what> · stop` |
| gate 4 | push · rewrite issue #<n>'s body with the merged spec · replace the draft's title and body · mark #<pr> ready · delete the run's cards — the human reviews and merges on GitHub | `approve · manual run first · change <what> · stop` |

A `Needs user` item is answered in the same reply; the orchestrator relays it to the implementer with the approval.

### Relayed question

An agent's `status=question` names `artifacts/<slug>/question.md` (shape: `spec-author.md` `## Questions`). Orchestrator never opens it in the viewer — the whole file is the prompt (`prompt-lines.sh` prints it; `show-file.sh` is for files the user approves, not reads in chat) — and asks once:

```
Question · <slug> · <gate or stage> · <agent> · <n> decisions
<the file, verbatim>
Answer: <1: a · 2: b · …> · other <text>
```

Answers go back to the **same** agent (`SendMessage`, never a respawn) in one message, numbered. A decision that depended on an answer arrives in the next `question.md`. A question without a real `Why you:` is sent back: the agent decides it and records `(chosen)`.

### Gate 1 — brief

`spec-author` grills the user and drafts the brief, `spec-diff.md` (its own file holds the grilling rules and the shape); user decides, orchestrator relays. Best effort: scope, scenarios, the user-visible flow and the edge cases the user cares about; everything else lands under `## Deferred` with an owner. Spawn `spec-author` with: user's goal verbatim, affected area(s), `artifacts/<slug>/`, and the issue number when the user named one ("take issue #n") — the request and the *Tracking issue* reuse candidate.

- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning. Named issue: store its body first, `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md`, and the brief it may already carry, `sh .claude/scripts/issue-request.sh <n> --spec > artifacts/<slug>/issue-spec.md` (delete it when empty).
- `status=question` → relay question and answer without comment.
- `status=ready` or `status=no-diff` (spec already covers the goal; the file names the violated requirements) → recommend the lane by `count=` (*Lane — fast or full*):
  - **full** recommended → show `spec-diff.md`, ask approval **and lane** in one prompt. Change requested → relay to the same agent. Approved → record `gate1` and `lane` on the parent card.
  - **fast** recommended → no stop: record `gate1: with-arch` and `lane: fast`; the brief is approved at the architecture stop.
  - Then *Architecture*.
- The agent stays alive for *Tracking issue*, *Spec changes* and gate 4.

### Architecture

`spec-architect` reads the code, grills the user on structure and drafts `architecture.md` (its own file holds the shape); user decides, orchestrator relays. Spawn it with: `artifacts/<slug>/spec-diff.md` path (non-behavior change: the goal verbatim instead), affected area(s), anything the user volunteered at gate 1.

- `status=question` → *Relayed question*.
- `status=split` → *Spec changes*, split.
- `status=behavior-change` (non-behavior run) → *Principles — implementation*.
- `status=no-arch` → record `arch: no-arch`; no stop; gate 2. Fast lane: the brief is approved at gate 2's stop, which always stops.
- `status=ready` → `show-file.sh`: `architecture.summary.md`, plus `spec-diff.md` when fast lane or its rev moved since gate 1's approval. One *Approval prompt* (fast lane: lane too). Change requested → relay to the same agent.
  - Approved → record `arch` (fast lane: `gate1` too) on the parent card; *Gate 2*.
- The agent stays alive until gate 2's approval, for the planner's `arch-gap`; a resumed run spawns a fresh one, which re-reads `architecture.md`.

### Gate 2 — plan and tracking issue

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md` path (none for a non-behavior change), `artifacts/<slug>/architecture.md` path (none on `no-arch`), affected area(s), anything the user volunteered. Nothing else; the agent explores the repo itself, starting from the architecture's code map. It writes `plan.md` (shape: `spec-planner.md`'s `## Output`) and `plan.summary.md`, edits `spec-diff.md` and assigns its fixed IDs.

- `status=question` → *Relayed question*.
- `status=arch-gap` → resume `spec-architect` with the reason verbatim; it may relay a question; its `ready` → resume the planner. No stop of its own.
- `status=split` → *Spec changes*, split.
- `status=behavior-change` (non-behavior run) → *Principles — implementation*.
- `status=ready` → fresh `spec-reviewer`, scope `plan`. Any finding, minors included → resume the planner with the `review.md` path, it revises the plan, fresh review — no user interaction; rounds and their end per *Verify before an approval stop*.
- The plan's `Stop` column (which stages stop for approval beyond the *Stage stop* rules) is settled at this stop: the user may tick or untick stages in the answer.
- `clean`, or `findings` with `count=0` → `spec-author` drafts `issue.md` (*Tracking issue*, no stop of its own; a non-behavior run spawns the author now with the goal verbatim, `plan.summary.md`, `artifacts/<slug>/`).
  - `show-file.sh` (*Agent hand-off*): `plan.summary.md`; `review.verdict.md` only when findings survived two rounds; `spec-diff.md` when the planner's `spec=` is above 0 (with `PROMPT_SINCE_REV`) or fast lane with `no-arch`. Never `issue.md`: the prompt's `Issue:` line is its one decision.
  - One *Approval prompt*: approve, plus mode (full lane), plus lane for a non-behavior change with `no-arch`.
  - **Fast lane, pre-authorized at the architecture stop:** no stop unless the verdict has a `Needs user` item, the plan outgrows the fast default, or the author answered `reuse` (an existing issue's body is about to be rewritten). Else one notice, no answer awaited, then *On approval*: `Gate 2 · <slug> · <Decide line> · issue new · starting s0 (pre-authorized — say stop to hold)`, followed by the `Confirm:` lines of `prompt-lines.sh` over `plan.summary.md` (and `spec-diff.md` with `PROMPT_SINCE_REV`, when `spec=` is above 0).

Mode, full lane only — unanswered = not approved:
- **Sequential** — one fresh `spec-implementer` runs every stage in plan order.
- **Parallel** — user gives max concurrent agents; fresh implementer per stage, separate worktrees. Waves capped at that number.

Default sequential; never infer concurrency from plan shape. Fast lane is always sequential.

**On approval, in order:**
1. File or rewrite the issue (*Tracking issue*); record `issue` on the parent card. Fast lane with `no-arch`: record `gate1` too.
2. Create the worktree: `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
3. Record `gate2`+`mode` (and `lane`, for a non-behavior change with `no-arch`) on parent card, move parent → `inprogress/`.
4. Create `open/<slug>.s<n>.md` per plan stage (`s0` only when the plan has one), `title`/`files`/`blocked-by` copied from the tree and `stop: yes|no` from `plan.summary.md`'s `Stop` column as the user settled it — `sh .claude/scripts/extract-section.sh '## Shared' artifacts/<slug>/plan.md` is the one plan read the orchestrator makes, for exactly those three fields. Parallel: also `open/<slug>.w<n>.md` per wave. Sequential: one wave-gate card for the run. Stage ids match plan ids.
5. Spawn the implementer, its brief naming the lane.

### Tracking issue

Drafted inside gate 2 once the plan review is clean (the IDs are fixed by then), approved at gate 2's stop (fast lane: pre-authorized at the architecture stop), filed on approval.

Orchestrator searches existing issues (`gh issue list --state all --search …`) for the same goal and passes candidate numbers — plus the one the user named at gate 1 — to `spec-author`, which writes `artifacts/<slug>/issue.md` (title + goal; shape: its file) and answers `status=reuse issue=<n> file=…` or `status=new file=…` in one turn. Reuse, never duplicate. On reuse the orchestrator stores the issue's original body once (unless gate 1 already did) with `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md` — the body as filed, or, when that body is one this workflow assembled (it carries `## Spec (rev`), the text inside its `<details>` block.

Filing:

```sh
sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md
# new
gh issue create --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
# reuse
gh issue edit <n> --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
```

**After filing, the body is rewritten only at gate 4**, from the spec synced from the branch, and when a split is filed (the status line changes). A meaningful spec change the user kept (*Spec changes*) gets a changelog comment instead: `spec-author` writes `issue-comment.md`, the orchestrator posts it, no further approval:

```sh
gh issue comment <n> --body-file artifacts/<slug>/issue-comment.md
```

Detail changes post nothing; the gate 4 rewrite carries them. The PR link is never added by hand: the PR's `Closes #<n>` line makes GitHub cross-link issue and PR. Every resume of `spec-author` for a changelog entry happens only while the issue is **filed**; before filing nothing is posted.

### Implement, stage by stage

#### Every stage

- One fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards and `lane=fast|full`; it reads its own rules (`.claude/agents/spec-implementer.md`, `AGENTS.md`).
- Not done until the plan's Verification method has run and its outcome is reported. Waiving it requires asking.
- **The draft PR body is the board, rendered** by `pr-body.sh <slug> draft` — regenerated and pushed with `gh pr edit <pr> --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)` on **every card move** once the PR exists.
- **The first push opens the draft PR** from that file; orchestrator records `pr` on the parent card, logs `pr=draft`. Every later push updates it; the user reviews there (*PR feedback*).

```sh
sh .claude/scripts/pr-body.sh <slug> draft > artifacts/<slug>/pr-draft.md
gh pr create --draft --title "$(head -1 artifacts/<slug>/pr-draft.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)
```

#### Fast lane

Per stage:

1. Green: the implementer commits, card → `inreview/`, answers `status=committed stage=s<n> delta=<n>` — no per-stage code review, no approval stop.
2. Orchestrator runs `gauntlet.sh`, pushes, logs `gauntlet=pass sha=<sha>` on the parent card. `delta>0` → fresh `spec-reviewer`, scope `delta s<n>` (*Spec changes*): `count>0` → resume the implementer with the `review.md` path, fixes committed on green, gauntlet, push, fresh `delta` review, rounds per *Verify before an approval stop*; a `Needs user` item → a *Delta* stop (*Approval prompt*). Then card → `done`, resume the implementer for the next stage. Red gauntlet → card → `inprogress/`, resume the implementer with the gauntlet line.
3. Last stage `done` → *PR feedback*, then *Gate 3 — review*: its branch review is the only code review of a fast-lane run.

Fixes in the fast lane (gate 3 findings, PR feedback, red gauntlet) are committed on green too: the implementer answers `status=committed`, the orchestrator runs `gauntlet.sh` and pushes before any re-review — a review diffs commits, never a dirty worktree.

#### Sequential

Full lane. Per stage:

1. Green: card → `inreview`, `status=inreview stage=s<n>`. Orchestrator runs `gauntlet.sh` and the per-stage review below.
2. Both clean → *Stage stop* rules decide: a rule fires → user approval stop; none fires → auto-continue (no stop). Either way the same implementer (resumed) commits, answers `status=committed`.
3. Orchestrator pushes the worktree (push is never the implementer's), re-runs `gauntlet.sh` on the pushed sha, logs `gauntlet=pass sha=<sha>` on the parent card, card → `done`.

#### Per-stage review

Full lane, sequential only; parallel's equivalent is the wave gate (parallel step 5). Every green stage, before its approval stop:
- Fresh `spec-reviewer`. Base ref = previous stage's commit (branch point for the first stage); scope = that stage id; `spec-reviewer.md`'s `## Axes, reported separately` and `## Spec changes` on one stage's diff.
- `clean`, or `findings` with `count=0` → approval stop, forward the gauntlet line; minors per *Verify before an approval stop*.
- `count>0` → card → `inprogress/`, resume implementer with `review.md` path and stage id, re-run `gauntlet.sh`, fresh reviewer; rounds per *Verify before an approval stop*. An unreviewed stage is never committed.
- **The last code stage**: review in scope `branch` instead — it is gate 3's review, and the stage's approval stop is gate 3's stop. Gate 3 re-runs only if code changed after it.

#### Stage stop

A green, clean-reviewed stage stops for approval only when one of these holds; otherwise it auto-continues. Checked in this order, the first hit is the stop's reason:

1. the verdict has a `Needs user` item — a meaningful spec change or an architecture deviation included
2. the stage's plan Verification goes beyond unit tests (demo TUI, real CSMS, device)
3. *PR feedback* fetched `threads>0`
4. it is the last code stage — its stop is gate 3's
5. the plan marks it `stop` (`plan.summary.md` `Stop` column, settled at gate 2)
6. its diff exceeds 400 changed lines or 8 files (`git diff --stat <base>...HEAD`)
7. checkpoint: three stages auto-continued since the last stop

`s0` never stops. Parallel mode: the rules apply to the wave gate (rule 6 on the wave's merged diff; rule 7 counts waves).

**Auto-continue** is one line in chat, no answer awaited, and the draft PR update:

```
Landed s2 · <slug> · review clean · gauntlet pass <cov> · pushed <sha> · spec detail: <IDs | none> · next: s3 (auto-continue — say stop to hold)
```

Log `auto=<rule-free>` or `stop=<rule n>` on the stage card. A user message while a stage is running is read before the next auto-continue; `stop` holds at the next green.

#### Parallel waves

Full lane only. One worktree+branch per agent, branched off the feature branch's tip at wave start (already containing every earlier wave's merged stages, so a dependent stage's worktree never starts without its dependencies' code): `git worktree add .claude/worktrees/<slug>-<n> -b <type>/<slug>-<n> <type>/<slug>`. One implementer each; never two agents in one worktree. Each wave:

1. Runnable stage cards (`blocked-by` all in `done/`) up to approved count. Wave-gate card → `inprogress/`.
2. One implementer per stage: its worktree path, its stage only, its own card's absolute path. It commits its stage on green in its own worktree (a merge needs a commit) — unreviewed, never leaves the worktree until step 4 clears it.
3. Wait for the whole wave; each card lands in `inreview/`, except one whose implementer answered `blocked reason=spec-delta: …` — it stays in `inprogress/`, logged `stopped: spec-delta`, its worktree removed unmerged (*Spec changes*, parallel).
4. Per card: `gauntlet.sh` in its worktree, fresh `spec-reviewer` in `stage s<n>` scope (base = wave's branch point). `clean` → merge into feature branch — **this merge is the new base** every later wave branches from — `gauntlet.sh` on the merged result, push the feature branch, card → `done/`, remove worktree. `findings` → card → `inprogress/`, same implementer resumes with `review.md` path, fixes, amends its stage commit, back to this step.
5. All `done` → wave gate → `inreview/`: fresh `spec-reviewer`, scope `wave w<n>`. `clean` → wave gate `done/`, then *Stage stop* rules on the wave — a rule fires → **stop: ask the user for approval before the next wave**; none → auto-continue — after *PR feedback* has run; its merge is the next wave's base, a fresh checkpoint. `findings`, red gauntlet, or merge conflict → stop, forward the status line, implicated stage cards → `inprogress/`; fresh implementer per card gets `review.md` path.

Merge conflict between two stages in a wave = dependency tree was wrong — report, fix the tree, never hand-resolve and continue. Mid-wave stop stops that wave only: finished branches still merge, the rest re-plans.

Gate 3 and the gate 4 spec sync are unchanged under parallelism: once, on the merged feature branch, never per agent.

### PR feedback

The user reviews on the draft PR, as inline comments on the diff. Once `pr` is on the parent card, **before every approval stop** (per-stage, wave gate, gate 3, gate 4) and whenever the user says to check the PR:

```sh
bash .claude/scripts/pr-feedback.sh fetch <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md
```

One status line: `pr=<n> state=… draft=… threads=<count> file=…`.

- `threads=0` → proceed to the approval stop.
- `threads>0` → this is a review pass, handled like `review.md` findings with `count>0`: log `feedback=<count>` on the parent card, implicated stage cards → `inprogress/` (a thread's `path:` names the file; the stage whose `files` list carries it owns it — unknown → the last stage), resume the same implementer (sequential, fast) or a fresh one per card (parallel) with the `pr-feedback.md` path. It fixes, fills each thread's `reply:` block, and answers its lane's status (*Fast lane*: `committed`; else `inreview`); then the usual review path on the touched stages (fast lane: gate 3's branch review). After the push:

```sh
bash .claude/scripts/pr-feedback.sh reply <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md s<n> <sha>
```

posts every filled reply block (`<stage> <sha>`: the stage and the pushed head), resolving `fixed` threads and leaving `declined` ones open for the user. A comment asking for behavior the spec doesn't cover is a spec change the implementer makes (*Spec changes*); the review classifies it. `status=blocked` (a comment is a question, or ambiguous) → relay `reason=` to the user as a question, never answer on the PR for them.

A thread the user resolves themselves, or one whose `reply:` they answer on the PR, drops out of the next fetch. Gate 4 does not run while `threads>0`.

### Spec changes

The spec is drafted best effort and settles as the run learns. Who changes it, and when the user is asked:

| Phase | Who changes it | Asks the user first |
|---|---|---|
| architecture | `spec-architect`, in `spec-diff.md`, local IDs | a change contradicting a scenario or a `decided (user)` line |
| gate 2 | `spec-planner`, in `spec-diff.md`, then fixes IDs | the same |
| after `s0` | `spec-implementer`, in the branch's `docs/specs/`, inside the stage commit | never — the reviewer classifies after |

Before `s0` every other change is recorded `decided (chosen)` and confirmed at the next stop (the *Approval prompt*'s `Confirm:` block).

**After `s0`.** The implementer logs `spec-delta` on its card and reports `delta=<n>`. The stage's reviewer (full lane: the per-stage or wave review; fast lane: a fresh `spec-reviewer` in scope `delta s<n>` whenever `delta>0`) classifies each change (`spec-reviewer.md` `## Spec changes`):
- **detail** → listed on the verdict and the auto-continue line; no decision.
- **meaningful** → a `Needs user` item: keep, revert, or split. Stage-stop rule 1 fires. The answer goes to the implementer.
  - **keep**, issue filed → resume `spec-author` for `issue-comment.md`, post it (*Tracking issue*), log `spec-keep=<IDs>` on the parent card. A kept architecture deviation → also a fresh `spec-architect` records it in `architecture.md` and its summary, no stop.
  - **revert** → the implementer reworks to the approved text, gauntlet, fresh review.
  - **split** → below; the implementer narrows the entry.

Parallel: a wave stage never edits `docs/specs/` — it ends `status=blocked stage=s<n> reason=spec-delta: …`. After the wave's merges, before the next wave branches: a fresh implementer in the run's worktree `.claude/worktrees/<slug>` (feature branch), briefed with the reason, lands the change as its own commit and logs `spec-delta` on the blocked stage's card; gauntlet, push; fresh `spec-reviewer` in scope `delta s<n>` (that stage's id, base = the feature branch tip before the commit). Clean → the blocked card → `open/`, runnable in the next wave with a fresh implementer.

**Split** — a change needed only for something outside the goal; the user's choice at a question or a `Needs user` item. Before `s0` the raising agent (architect, planner) has already narrowed the entry when it answers `status=split reason=…`; after `s0` the reviewer only raised it, and the implementer narrows it last (below). Orchestrator resumes `spec-author` with the reason verbatim (after `s0`: the verdict item and the `review.md` path); it writes `split-issue.md`. Orchestrator files it (`gh issue create`, as in *Tracking issue*) with `Split from #<issue>` appended as its last line — at once if the tracking issue is filed, else right after gate 2 approval step 1 — appends it to the parent card's `split:`, reassembles and edits the issue body, posts one notice line `Split · <slug> · <IDs> · #<m> filed`, then resumes the architect or planner that raised it — after `s0`, the implementer with `split <ID>: narrow it`, gauntlet, the usual review. The split issue's body is `split-issue.md` with the status line `Split from #<issue> · no spec yet` prepended. The new issue is its own later run.

**Fold in** — the user's explicit override of a split: the raising agent rewrites the `Goal:` line to the widened scope (after `s0`: it is a meaningful change, kept).

Gate 3's reviewer checks every branch spec change was classified; gate 4's sync makes `spec-diff.md` the final spec, never one the orchestrator composes.

### Gate 3 — review

Before proposing a PR, whole-branch pass — cross-stage bugs, spec drift across stages, scope creep:
- Fresh `spec-reviewer`, scope `branch`. Give it base ref, artifact dir, worktree path, all stage ids; it reads its own rules (`AGENTS.md`, `spec-reviewer.md`'s `## Axes, reported separately`). Already run as the last code stage's review (*Per-stage review*) → skip to gate 4.
- Orchestrator runs `gauntlet.sh` on the branch, forwards both status lines.
- `count>0` → as *Per-stage review*, the same implementer (fast lane, sequential) or one fresh implementer per implicated card (parallel), then a fresh branch review. Findings needing a user decision are relayed as questions, never fixed unasked.
- Log `review=clean` or `review=<n>-resolved` on the parent card once the branch review is clean (the PR status line reads it). Minors (*Verify before an approval stop*) are resolved before `spec-author` writes `pr.md`; gauntlet re-run, `gauntlet=pass sha=<sha>` logged on the parent card.
- **Fast lane:** no stop here — clean goes straight to gate 4, whose stop shows the verdict.
- **Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage id, and rewrites `review.verdict.md`. Parent card → `inreview/` when review starts.

### Gate 4 — pull request

- Gauntlet + gate 3 clean, *PR feedback* at `threads=0`.
- `spec-author`, given the worktree path, syncs `spec-diff.md` from the branch's `docs/specs/` and writes `artifacts/<slug>/pr.md` in **full form** (content rules in its file; `.github/PULL_REQUEST_TEMPLATE.md` carries the same sections for human PRs). Orchestrator assembles both bodies:

```sh
sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md
sh .claude/scripts/pr-body.sh <slug> full > artifacts/<slug>/pr-full.md
```

- **One stop**: `show-file.sh` on `pr.md` (fast lane: plus `review.verdict.md`, per *Agent hand-off*); the *Approval prompt* offers **approve** or **manual run first** — the user may want one; don't pre-empt it. Approved → push, rewrite the issue body, replace the draft's title and body, mark ready, log `pr=ready`:

```sh
gh issue edit <n> --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
gh pr edit <pr> --title "$(head -1 artifacts/<slug>/pr-full.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-full.md)
gh pr ready <pr>
```

- No draft exists (run predates this rule, or resumed from a board that lacks `pr`): `gh pr create` with the same title and body file instead of `edit`+`ready`.
- **The run ends here.** The PR is marked ready; the human reviews and merges in the GitHub UI, never the orchestrator. Every card of the run is deleted (`done/` was never the archive); `artifacts/<slug>/` and the worktree stay until *After merge*.
- CI fails after mark-ready → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`; recreate the implicated stage card from the plan in `inprogress/`, a fresh implementer fixes, gauntlet, push, cards deleted again.

### After merge

When the user says the PR is merged (or `pr-feedback.sh fetch` reports `state=merged` on a resume):

```sh
git worktree remove .claude/worktrees/<slug>
git worktree list   # nothing under .claude/worktrees/ should remain
rm -r .claude/tasks/artifacts/<slug>
```

Per-wave worktrees are removed at wave end; this sweep catches stragglers. No approval stop: the artifacts were a run's working files, the issue and PR hold everything that outlives it.

### Resume an interrupted run

Cards outside `open/`+`done/`, no agent running = session died mid-run. Resume triggered by the user or `/spec-feature`, never automatic.

No worktree on the card → died during gate 1, architecture or gate 2, nothing on disk to reconcile — resume the conversation from `spec-diff.md`/`architecture.md`/`plan.md`'s last state (`arch` set → gate 2). `issue` set but no worktree → gate 2 was approved: continue at its step 2, never re-file. Past gate 2 → table below. **Any resumed implementation spawns a fresh agent**, its brief naming the card's `lane`.

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `issue=<n>` | `issue-view.sh <n>` — open; a comment per `spec-keep=` on the parent card | missing comment: post `issue-comment.md` (*Tracking issue*) |
| `pr=<n>` | `pr-feedback.sh fetch <n> …` — its status line's `state=`/`draft=` | closed/merged: run is over; missing: draft never opened, the next push opens it |
| `gauntlet=pass` | `gauntlet.sh` at that sha | card overstated state |
| stage `done` | `git branch --contains` vs feature branch | never merged; downstream plans a lie |
| PR `## Stages` | the PR body vs `pr-body.sh <slug> draft` | body behind the board: a move never rendered, `gh pr edit` with the fresh render |

Report differences first. Agree → resume. Disagree → stop and report.

Clean reconcile → resume only no-approval work (respawn implementers for approved stages, merge finished branches, run wave gates); halt at the first gate needing the user. Recorded `gate1`/`arch`/`gate2` approvals stay valid — the fast lane's pre-authorization too: `arch` set, `lane: fast`, no `gate2` → plan, then gate 2's *On approval* without a stop. A resumed run spawns every agent fresh (the architect, the author and the planner re-read their files).
