# AGENTS.workflow.md

Gate/task-board mechanics for the orchestrator. Subagents (spec-author/architect/planner/implementer/reviewer) never read it.

### Principles

Every run, `/spec-request` included.

- Replaces any generic workflow skill (`/workflow`); `docs/specs/` is already the PRD and design record.
- Branch off `main`, never commit to `main`. `<type>/<slug>`, type ∈ {`feat`, `fix`, `docs`}. **Enforced:** `.claude/scripts/hook-guard-shell.sh` denies `git commit` on `main` and `git push` targeting `main`.
- **Orchestrator never changes spec or code and never reads spec, code, or any agent-written file.** It spawns agents, relays one question at a time between agent and user, runs git/`gh` plumbing (worktree add/remove, merge, push, `gh … --body-file`), runs `.claude/scripts/gauntlet.sh`, moves cards. Every other output is an agent's file (*Agent hand-off*).
- **Gate 1 is a best-effort grilling, drafted by `spec-author`.** Existing spec + the request's user stories, nothing about current code. It settles scope, scenarios and the edge cases the user cares about; every other point is deferred to the agent that settles it, never anticipated.
- **Issue and PR are the orchestrator's alone**, from files scripts assemble (`issue-body.sh`, `pr-body.sh`), title = line 1 with its `# ` stripped, body = the rest, never retyped. Issue = goal, requirements, scenarios; PR = everything about how; neither names a stage or an agent. No agent but `spec-author` learns an issue or PR number exists — spawn briefs never carry one.
- **An agent's self-reported verification is not verification.** Re-run the tools.
- **A split of an area whose spec costs real context is `spec-author`'s to propose** at gate 1 (rules in its file) and the user's to approve, never silent.

### Principles — implementation

Gate 2 onward.

- **Non-behavior change (`AGENTS.md` `## Workflow`) skips gate 1** and starts at *Architecture*: no `spec-diff.md`; parent card records `gate1: skipped`. Architecture or planning finds a behavior change (`status=behavior-change`) → stop, run gate 1 with the reason, resume the same agent with the `spec-diff.md` path.
- Gate 2 onward delegates to agents. **All agents Sonnet or better.**
- **The draft PR is the user's review surface** (*Every stage*; promoted at *Gate 4 — pull request*). Feedback re-enters the run only through *PR feedback*.
- **One git worktree per run**, `.claude/worktrees/<slug>` — inside project dir (agent-reachable), gitignored. Created only once gate 2 is approved (first thing to touch disk); removed by *Clean up* at the end of gate 4.
- **One implementer at a time**, stages in plan order.
- **The run ends when the PR is ready and CI is green** — then *Clean up* removes the worktree and local branch and archives the board. Merge happens outside the workflow.
- **Every state change moves a task-board card.** State living only in conversation dies with the session.
- **Every stop is logged** — before asking, the orchestrator logs `stop kind=<question|approval|stage|blocked|delta|replan>` to the parent card (`approval`: gate, architecture, epic, delta and re-plan stops). `metrics.sh` counts them at the end of the run. A workflow change that adds stops or agents names its expected effect on `metrics.log` in its PR.

### Lane — fast or full

The lane sets how many approval stops the run makes. An epic run (*Epic*) has no lane: it stops at its brief and at the epic stop; each slice picks its own lane. Proposed by size when the first agent output of the run is ready; the user settles it at the run's first approval stop, recommended lane first.

Recommended default, by size:
- **fast** — `no-diff`, or ≤ 3 `R`/`E` entries in the brief (gate 1 `count=`); non-behavior change: `no-arch`, or ≤ 3 new or changed components (architecture `count=`).
- **full** — anything bigger.

| | fast | full |
|---|---|---|
| Brief (gate 1) | no stop of its own — approved at the architecture stop | own stop, settles the lane |
| Architecture | one stop: brief + architecture + lane; approving pre-authorizes gate 2's *On approval* steps | own stop |
| Gate 2 | no stop unless the plan review leaves a `Needs user` item or the plan outgrows the fast default; else a notice | plan summary + issue draft |
| Stages | *Fast lane* | *Full lane*, stops per *Stage stop* |
| End | one stop, *Gate 4 — pull request* | *Gate 3 — review* stop, then *Gate 4 — pull request* stop |

Recommended full, user picks fast at the brief stop: the architecture stop is fast's (without the brief). The fast default for a plan is at most 2 stages besides `s0` (*Gate 2*). The user may switch lanes at any later stop; a lane never switches silently.

### Verify before an approval stop

By running, never by reading:
- **Gauntlet** — `sh .claude/scripts/gauntlet.sh <worktree> .claude/tasks/artifacts/<slug>` in the agent's worktree: one status line in context, full output in `artifacts/<slug>/gauntlet.log`. The full set runs once per tree: a pass is cached by the worktree's tree, so the same tree again (committed, pushed) answers `gauntlet=pass … cached` at once. The implementer runs only fmt, clippy and its crates' tests; the full set is this one. `GAUNTLET_FORCE=1` reruns a cached tree.
- **A fresh `spec-reviewer` per scope** — never the planner or implementer: one sharing the author's context reproduces its blind spots. A follow-up pass in the same scope (plan, a stage, a delta) **resumes that reviewer** (`SendMessage`) with the `review.md` path and `follow-up` (`spec-reviewer.md` `## Follow-up pass`). Fresh anyway when that reviewer is gone (a resumed session) and on every gate 3 `branch` pass — the last check before the PR gets fresh eyes. Plan scope: before resuming the planner with findings, the orchestrator snapshots `cp artifacts/<slug>/plan.md artifacts/<slug>/plan.prev.md`, so the follow-up diffs the revision itself instead of trusting the planner's account.
- **Minors never wait for the user.** The verdict names where they go: `Minors (folded into s<n+1>)` when a stage follows, `Minors (fixed before commit)` when none does — a last stage, a branch review. Folded: the orchestrator logs each as `minor=<verdict line>` on the next stage's card and repeats them in the implementer's brief. Fixed before commit: the implementer is resumed with them at once, fixes, gauntlet, no re-review, then the stage's commit (fast lane: commit on green). Only `count>0` (blockers + majors) loops, and at most **two** fix-and-review rounds per scope: findings still open after the second round go to the user as `Needs user` items on the verdict.

### Task board

Directory a card sits in **is** its state:

```
.claude/tasks/
  open/  inprogress/  inreview/  done/   cards move between these
  metrics.log          one line per finished run (metrics.sh), never cleaned up
  archive/<slug>/      cards/ + artifacts/ of a finished run (run-cleanup.sh), never cleaned up
  artifacts/<slug>/
    spec-diff.md       gate 1 brief, then the run's spec until s0 — local IDs until gate 2; synced from the branch at gate 4
    architecture.md    components, structure, interactions, public surface, code map — the plan's target
    architecture.summary.md  architecture decisions the user approves, capped
    plan.md            gate 2 stages, steps, dependency tree — the implementer's contract
    plan.summary.md    gate 2 decisions the user approves, capped
    plan.prev.md       plan.md as the last plan review saw it, for the follow-up pass
    review.md          review findings + history, keyed by stage id, append-only
    review.verdict.md  open findings per stage, rewritten every pass, capped
    gauntlet.log       full build/test/lint output of the last gauntlet.sh run
    gauntlet.pass      tree hash + coverage of the last passing tree, gauntlet.sh's cache
    question.md        an agent's batch of decisions for the user (*Relayed question*)
    slices.md          epic run: the ordered slices, each later its own issue (*Epic*)
    slice-<n>.md       epic run: slice n's assembled issue body
    epic.md            slice run: the epic issue's spec and skeleton architecture (*Epic*)
    issue.md           title + goal + out of scope (*Tracking issue*)
    issue-live.md      the live issue body, fetched for the status-line rewrite
    issue-status.md    the live body with its status line rewritten
    request.md         the request issue's original body, stored once (gate 1 or reuse)
    issue-comment.md   changelog comment of the latest user-approved spec change after filing
    split-issue.md     an issue split off (*Spec changes*)
    follow-up-<k>.md   gate 4: an issue for something the run noticed outside its goal
    pr.md              gate 4 PR title + body (*Gate 4 — pull request*)
    pr-feedback.md     unresolved PR review threads (*PR feedback*)
```

`artifacts/<slug>` everywhere in this file and the agents' files is shorthand for `.claude/tasks/artifacts/<slug>` in the **main checkout**: every spawn brief passes its absolute path (an agent in a worktree would resolve a relative one inside the worktree), every orchestrator command runs from the main checkout's root.

Directories tracked; cards gitignored. Cards live in **main checkout only** — a worktree agent gets its own card's id and the board root and writes only that card (`spec-implementer.md` says how). No per-worktree board copy.

| Card | File | Owner |
|---|---|---|
| parent | `<slug>.md` | orchestrator — one per run |
| stage | `<slug>.s<n>.md` | the implementer working that stage |

Cards are agent-only: YAML frontmatter + a log of terse field=value tokens, no prose. Lines are appended by `card-log.sh`, never rewritten — crash mid-write costs one truncated line, not the file.

```
---
id: <slug>.s3
parent: <slug>
title: <title>
blocked-by: [<slug>.s2]
files: [src/x.rs, tests/x.rs]
branch: <type>/<slug>
worktree: .claude/worktrees/<slug>
---
2026-01-02T14:02 spawn agent=impl
2026-01-02T14:12 checks=pass crates=ferrowl-modbus
```

Stage frontmatter `title` = the stage's display name, the text after `## Stage s<n>: ` in its `plan.md` heading.

Log line = `<ISO minute> <event> [key=value …]`; the implementer's own file carries the stage-event vocabulary. Every line on any card is written by `sh .claude/scripts/card-log.sh <card-id> <event> [k=v …] [-- <text>]` (vocabulary in the script; `hook-guard-shell.sh` denies a raw `>>`/`tee -a` onto a card); "log <event>" in this file means that call. A mid-run addition to a stage's `files` is logged on its card as `files+=<path>[, <path>] user=approved reason=…`, a minor handed to another stage as `minor-moved=<verdict line>` (on the stage that gave it up). `card-log.sh` stamps every line with `date -u +%FT%H:%M` — UTC on every card, so a resumed session can order events across cards.

Parent frontmatter: `kind: epic|slice` (absent on a plain run), `epic` (slice run: the epic's issue number), `slices: [<n>, …]` (epic run: the child issues, in slice order), `issue`, `pr` (number, set when the draft opens), `branch`, `lane: fast|full` (set at the first approval stop), `gate1` approval date, `with-arch` (fast lane, until the architecture stop) or `skipped`, `arch` approval date or `no-arch`, `gate2` approval date, `split: [<n>, …]` (issues split off), `upstream: [<repo>#<n>, …]`, `artifacts`. `card-field.sh <slug> <field>` reads one. Never the goal or normative text — issue holds the goal, `artifacts/` the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review (fast lane: already committed) | committed, pushed and orchestrator-verified; full lane also reviewed and approved or auto-continued |
| parent | gate 1, architecture or gate 2 pending | implementing | gate 3/4, until *Clean up* archives it | — never: the run ends in *Clean up* |

Rules:
- **No agent writes its own `done`** — the orchestrator's move (table above).
- **Runnable** = every `blocked-by` id in `done/`.
- No `blocked/` directory — blocking derives from `blocked-by`.
- Card is intent, git is fact — see *Resume an interrupted run*. Cards end in *Clean up*.

### Agent hand-off

Every agent writes its full output to a file and ends its turn with a **status line** — nothing else.

```
status=<token> [file=<path>] [summary=<path>] [stage=s<n>] [issue=<n>] [reason=<one line>] [count=<n>] [delta=<n>] [user=<n>] [spec=<n>]
```

| Agent | `status` tokens | `file` | `summary` |
|---|---|---|---|
| spec-author | `question` (`count=` decisions) · `ready` (`count=` `R`+`E` entries at gate 1; `size=epic reason=` when the brief is too big for one run) · `no-diff` (`count=0`) · `reuse` (`issue=`) · `new` — both with `file=` `issue.md` | `artifacts/<slug>/spec-diff.md`, `question.md`, `issue.md`, `issue-comment.md`, `split-issue.md`, `pr.md` | none — `prompt-lines.sh` quotes each file |
| spec-architect | `question` (`count=` decisions) · `ready` (`count=` new + changed components; `slices=` path when it wrote an epic) · `no-arch` · `split` (`reason=`) · `behavior-change` (`reason=`, non-behavior run) | `artifacts/<slug>/architecture.md`, `question.md`, `slices.md` (epic); edits `spec-diff.md` | `artifacts/<slug>/architecture.summary.md` |
| spec-planner | `question` (`count=` decisions) · `arch-gap` (`reason=`) · `split` (`reason=`) · `behavior-change` (`reason=`, non-behavior run) · `ready` (`count=` stages, `spec=` `spec-diff.md` revs it wrote) | `artifacts/<slug>/plan.md`, `question.md`; edits `spec-diff.md` | `artifacts/<slug>/plan.summary.md` |
| spec-implementer | `inreview` · `committed` (`sha=`, `delta=`; both `delta=` spec changes) · `question` (`count=` decisions) · `blocked` (`reason=`) | stage card (`stage=`), `question.md` | none — the card is the summary |
| spec-reviewer | `clean` · `findings` (`count=` blockers+majors, `user=` `Needs user` items, `stage=` list) · `blocked` | `artifacts/<slug>/review.md` | `artifacts/<slug>/review.verdict.md` |

`file` is lossless, for the next agent: as long as it must be. `summary` is for the user: decisions and open findings only, rewritten whole on every pass, capped at 25 lines / 2 KB (`show-file.sh` refuses a larger one and exits 3 — the agent trims it).

- `question` = a batch of decisions in `artifacts/<slug>/question.md` (*Relayed question*).
- An agent returning more than the status line is told to move the rest into its file and answer again.
- Every approval of `spec-diff.md` logs `spec-rev=<N>` on the parent card (the rev `show-file.sh` named). A later stop showing it again runs `show-file.sh` with `PROMPT_SINCE_REV=<that N>`, so `Confirm:` carries only the `(chosen)` lines written since, and the `[spec <ID>]` decision lines of `plan.md`/`architecture.md` whose rev is later.
- A status line naming a file the user must approve (`spec-diff.md`, `pr.md`, the `summary=` path — `architecture.summary.md`, `plan.summary.md`, `review.verdict.md` when it has a `Needs user` item): orchestrator runs `sh .claude/scripts/show-file.sh <path> [<path> ...]` before asking — every file of one approval stop in a single call; it opens each outside the context and prints, per file, one line naming how it was shown plus that file's quotable prompt lines (`prompt-lines.sh`), for the *Approval prompt*.

Reviewer scope tokens: `plan` (gate 2), `plan from s<n>` (*Re-plan*), `stage s<n>`, `delta s<n>` (one stage's spec changes, *Spec changes*), `branch` (gate 3).

### Approval prompt

Every approval stop is one message in this fixed shape, composed from `show-file.sh`'s quotable lines, the agents' status-line tokens and the stop's fixed lines from its section. A block absent from this stop is omitted; order is fixed. The user pulls one heading of the full file with `extract-section.sh` for the *why*.

```
<Gate <n> | Architecture | Epic | Delta s<n> | Re-plan from s<n>> · <slug> · <files shown, with rev> [· fast lane]
Goal: <spec-diff.md's Goal line>
Scenarios: <spec-diff.md's count and S-IDs>
Entries: <spec-diff.md's counts and IDs>
Deferred: <spec-diff.md's count per owner>
Size: epic recommended — <the author's reason>                                       ← only when the author answered size=epic
Architecture: <architecture.summary.md's Decide line>
Interfaces: <architecture.summary.md's Interfaces line>
Surface:
- <architecture.summary.md's public-surface lines>
Slices:
- <slices.md's one line per slice>
Plan: <plan.summary.md's Decide line>
Spec since arch: <plan.summary.md's Spec-since-architecture line>
Stops:
- <stages the plan marks stop, with why>
Issue: <new | reuse #<n>> — <issue.md's title>                                        ← new/reuse from the author's status line
Split: <split-issue.md's title>
Follow-ups:
- <Follow-up <k>: title, one per follow-up-<k>.md>                                 ← all filed unless the answer narrows them
Verification:
- <plan.summary.md's lines>
Confirm:
- <prompt-lines.sh's single Confirm block over the shown files>                       ← or `Confirm: nothing flagged`
Review: <verdict line>
Not checked: <verdict's Not checked line>
Spec: <n of m pinned …>
Spec changes: <verdict's detail and need-you IDs>
Minors (folded into <where>):
- <verdict's minor lines>                                                            ← information, not a decision
Needs user:
- <verdict's question and options>
Approving will:
- <the stop's fixed list, in its section>
Answer: <the stop's fixed answer line, in its section>
```

Each stop's fixed `Approving will:` list and `Answer:` line are in that stop's section (**Stop:** bullet).

A `Needs user` item is answered in the same reply; the orchestrator relays it to the implementer with the approval.

### Relayed question

An agent's `status=question` names `artifacts/<slug>/question.md` (shape: `spec-author.md` `## Questions`). Orchestrator never opens it in the viewer — the whole file is the prompt (`prompt-lines.sh` prints it; `show-file.sh` is for files the user approves, not reads in chat) — and asks once:

```
Question · <slug> · <gate or stage> · <agent> · <n> decisions
<the file, verbatim>
Answer: <1: a · 2: b · …> · other <text>
```

Answers go back to the **same** agent (`SendMessage`, never a respawn) in one message, numbered. A decision marked `Assumes: <n> = <option>` is void when the user answered `<n>` otherwise; the agent asks it again in its next `question.md`, as it does a decision that depended on an answer and waited. A question without a real `Why you:` is sent back: the agent decides it and records `(chosen)`.

### Gate 1 — brief

`spec-author` grills the user and drafts the brief, `spec-diff.md` (its own file holds the grilling rules and the shape); user decides, orchestrator relays. Best effort: scope, scenarios, the user-visible flow and the edge cases the user cares about; everything else lands under `## Deferred` with an owner. Spawn `spec-author` with: user's goal verbatim, affected area(s), `artifacts/<slug>/`, and the issue number when the user named one ("take issue #n") — the request and the *Tracking issue* reuse candidate.

- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning. Named issue: store its body first, `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md`, and the brief it may already carry, `sh .claude/scripts/issue-request.sh <n> --spec > artifacts/<slug>/issue-spec.md`; delete either file when empty. `sh .claude/scripts/issue-request.sh <n> --epic` prints a number → the issue is a slice: *Epic*, **Slice run**, first.
- `status=question` → relay question and answer without comment.
- `status=ready size=epic` → always a stop, whatever the lane would be: show `spec-diff.md`, the prompt carries `Size:`. `approve epic` → record `kind: epic` and `gate1`; *Epic*. `approve single` → as below, with the lane given.
- `status=ready` or `status=no-diff` (spec already covers the goal; the file names the violated requirements) → recommend the lane by `count=` (*Lane — fast or full*):
  - **full** recommended → show `spec-diff.md`, ask approval **and lane** in one prompt. Change requested → relay to the same agent. Approved → record `gate1` and `lane` on the parent card.
  - **fast** recommended → no stop: record `gate1: with-arch` and `lane: fast`; the brief is approved at the architecture stop.
  - Then *Architecture*.
- **Stop:** approving will settle the lane · hand the brief to the architect. Answer: `approve [full|fast] · change <what> · stop`; with `size=epic`: `approve epic · approve single [full|fast] · change <what> · stop`.
- The agent stays alive for *Tracking issue*, *Spec changes* and gate 4.

### Architecture

`spec-architect` reads the code, grills the user on structure and drafts `architecture.md` (its own file holds the shape); user decides, orchestrator relays. Spawn it with: `artifacts/<slug>/spec-diff.md` path (non-behavior change: the goal verbatim instead), affected area(s), anything the user volunteered at gate 1.

- `status=question` → *Relayed question*.
- `status=split` → *Spec changes*, split.
- `status=behavior-change` (non-behavior run) → *Principles — implementation*.
- `status=no-arch` → record `arch: no-arch`; no stop; gate 2 (fast lane: its stop approves the brief).
- `status=ready slices=…` → the user chose an epic at the architect's size guard: record `kind: epic`; *Epic* step 3.
- `status=ready` → `show-file.sh`: `architecture.summary.md` and `architecture.md` (the interface sketches are there; the viewer shows them, the orchestrator never reads them), plus `spec-diff.md` when fast lane or its rev moved since gate 1's approval. One *Approval prompt* (fast lane: lane too). Change requested → relay to the same agent.
  - Approved → record `arch` (fast lane: `gate1` too) on the parent card; *Gate 2*.
- **Stop, full:** approving will hand brief and architecture to the planner. Answer: `approve · change <what> · stop`; first stop of a non-behavior run: `approve [full|fast] · change <what> · stop`.
- **Stop, fast:** approving will approve the brief (when pending) · record the lane · hand both to the planner · run gate 2's *On approval* without a stop once the plan review is clean · next stop: gate 4. Answer: `approve · switch to full · change <what> · stop`.
- The agent stays alive until gate 2's approval, for the planner's `arch-gap`; a resumed run spawns a fresh one, which re-reads `architecture.md`.

### Epic

A feature too big for one run: the author answered `size=epic`, or the architect's size guard tripped. The epic run settles the shared core and cuts the feature into slices; it writes no code and has no plan, worktree or PR. Each slice is later a normal run on its own issue.

1. **Brief** — gate 1 as usual; its stop approves `epic`. Or the architect's size guard turned a behavior run into one: record `kind: epic`, drop `lane` from the card, go to step 3; the epic stop approves the brief too when `gate1` is not yet a date (fast lane).
2. **Skeleton and slices** — spawn `spec-architect` with the brief and `mode=epic`. It writes `architecture.md` and its summary for the **skeleton** only (the core abstractions every slice builds on, with their `## Interfaces`) and `slices.md` (shape: its file). Questions relayed as usual; it answers `status=ready … slices=artifacts/<slug>/slices.md`.
3. **Epic issue draft** — `spec-author` drafts `issue.md` (*Tracking issue*: existing issues searched, `reuse` or `new`).
4. **Epic stop** — `show-file.sh` on `architecture.summary.md`, `architecture.md`, `slices.md`, plus `spec-diff.md` when the brief is not yet approved or its rev moved since (`PROMPT_SINCE_REV`); one *Approval prompt* with `Slices:`, `Issue:` and the `Confirm:` lines — among them every brief ID no slice or two slices claim. Approved → record `gate1` (when still pending) and `arch`.
   - **Stop:** approving will file the epic issue (or rewrite #<n>) · file <n> slice issues in order · link them from the epic · clean up — no code, no worktree. Answer: `approve · change <what> · stop`.
5. **On approval, in order** — no further stop:
   1. File or rewrite the epic issue from `issue-body.sh <slug>` (*Tracking issue*); record `issue`.
   2. Per slice, in `slices.md` order: `sh .claude/scripts/issue-body.sh <slug> --slice <n> > artifacts/<slug>/slice-<n>.md`; an open issue already titled exactly as its line 1 (`gh issue list --state open --search "<title> in:title"`) is adopted, never filed twice — else `gh issue create` from it (title = line 1, body = the rest); then append the number to the parent card's `slices:` — a later slice's `Depends on` links it.
   3. Reassemble and edit the epic issue's body: its `## Slices` list now links every child.
   4. `sh .claude/scripts/metrics.sh <slug>`, then *Clean up* — no worktree to remove; cards and artifacts go.
   5. One notice line: `Epic · <slug> · #<e> · slices #a, #b, … filed · start with /spec-feature on #a`.

The human closes the epic once its last slice merges.

**Slice run** — `/spec-feature` on a child issue ("take issue #n"), a normal run otherwise.
- **Gate 1 board:** record `kind: slice` and `epic: <e>`; store the slice's status line, `sh .claude/scripts/issue-request.sh <n> --slice-line > artifacts/<slug>/slice-line.md` (`issue-body.sh` keeps it on every rewrite), and the epic as it stands, `issue-request.sh <e> --spec > artifacts/<slug>/epic.md` then `issue-request.sh <e> --comments >> artifacts/<slug>/epic.md`. Name `epic.md`'s path in every spawn brief of the run except the implementer's.
- **Kept skeleton change** — its `issue-comment.md` is posted on the epic too: `gh issue comment <epic> --body-file …`, the card's `epic`.
- **Split** — a slice's split-off issue gets `**Part of epic** #<e> · split from #<slice>` prepended as its first line, and a one-line comment on the epic names it.
- Slices whose `Depends on` don't chain may run at the same time; gate 4's ID collision check covers them.

### Gate 2 — plan and tracking issue

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md` path (none for a non-behavior change), `artifacts/<slug>/architecture.md` path (none on `no-arch`), affected area(s), anything the user volunteered. Nothing else; the agent explores the repo itself, starting from the architecture's code map. It writes `plan.md` (shape: `spec-planner.md`'s `## Output`) and `plan.summary.md`, edits `spec-diff.md` and assigns its fixed IDs.

- `status=question` → *Relayed question*.
- `status=arch-gap` → resume `spec-architect` with the reason verbatim; it may relay a question; its `ready` → resume the planner. No stop of its own.
- `status=split` → *Spec changes*, split.
- `status=behavior-change` (non-behavior run) → *Principles — implementation*.
- `status=ready` → fresh `spec-reviewer`, scope `plan`. Any finding, minors included → snapshot `plan.prev.md`, resume the planner with the `review.md` path, it revises the plan, the same reviewer runs a follow-up pass — no user interaction; rounds and their end per *Verify before an approval stop*.
- The plan's `Stop` column (which stages stop for approval beyond the *Stage stop* rules) is settled at this stop: the user may tick or untick stages in the answer.
- `clean`, or `findings` with `count=0` → `spec-author` drafts `issue.md` (*Tracking issue*, no stop of its own; a non-behavior run spawns the author now with the goal verbatim, `plan.summary.md`, `artifacts/<slug>/`).
  - `show-file.sh` (*Agent hand-off*): `plan.summary.md`; `review.verdict.md` only when findings survived two rounds; `spec-diff.md` when the planner's `spec=` is above 0 (with `PROMPT_SINCE_REV`) or fast lane with `no-arch`. Never `issue.md`: the prompt's `Issue:` line is its one decision.
  - One *Approval prompt*: approve, plus lane for a non-behavior change with `no-arch`.
  - **Fast lane, pre-authorized at the architecture stop:** no stop unless the verdict has a `Needs user` item, the plan outgrows the fast default, or the author answered `reuse` (an existing issue's body is about to be rewritten). Else one notice, no answer awaited, then *On approval*: `Gate 2 · <slug> · <Decide line> · issue new · starting s0 (pre-authorized — say stop to hold)`, followed by the `Confirm:` lines of `prompt-lines.sh` over `plan.summary.md` (and `spec-diff.md` with `PROMPT_SINCE_REV`, when `spec=` is above 0).

- **Stop:** approving will file the issue (or rewrite #<n>) · create worktree `<path>` on branch `<branch>` · create <n> stage cards · spawn the implementer at <first stage> (fast: commits and pushes on green, no per-stage stop) · next stop: <per-stage s<n> | gate 4>. Answer: `approve [stop: s1,s3] · switch lane · change <what> · stop`.

**On approval, in order:**
1. File or rewrite the issue (*Tracking issue*); record `issue` on the parent card. Fast lane with `no-arch`: record `gate1` too.
2. Create the worktree: `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
3. Record `gate2` (and `lane`, for a non-behavior change with `no-arch`) on parent card, move parent → `inprogress/`, then the status-line update (*Tracking issue*).
4. Create `open/<slug>.s<n>.md` per plan stage (`s0` only when the plan has one), `title`/`files`/`blocked-by` copied from the tree and `stop: yes|no` from `plan.summary.md`'s `Stop` column as the user settled it — `sh .claude/scripts/extract-section.sh '## Shared' artifacts/<slug>/plan.md` is the one plan read the orchestrator makes, for exactly those three fields. Stage ids match plan ids.
5. Spawn the implementer, its brief naming the lane.

### Tracking issue

Drafted inside gate 2 once the plan review is clean (the IDs are fixed by then), approved at gate 2's stop (fast lane: pre-authorized at the architecture stop), filed on approval.

Orchestrator searches existing issues (`gh issue list --state all --search …`) for the same goal and passes candidate numbers — plus the one the user named at gate 1 — to `spec-author`, which writes `artifacts/<slug>/issue.md` (title + goal; shape: its file) and answers `status=reuse issue=<n> file=…` or `status=new file=…` in one turn. Reuse, never duplicate. On reuse the orchestrator stores the issue's original body once (unless gate 1 already did) with `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md` — the body as filed, or, when that body is one this workflow assembled (it carries `## Scenarios` or `## Spec (rev`), the text inside its `<details>` block.

Filing:

```sh
sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md
# new
gh issue create --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
# reuse
gh issue edit <n> --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
```

**After filing, the body is never rewritten**; only its status line (line 1) changes, at gate 2 *On approval* step 3 (`in progress`), when a split is filed (`Split off`) and at `ci=0` (`done`):

```sh
gh api repos/{owner}/{repo}/issues/<n> --jq .body > artifacts/<slug>/issue-live.md
sh .claude/scripts/issue-body.sh <slug> --status artifacts/<slug>/issue-live.md > artifacts/<slug>/issue-status.md
gh issue edit <n> --body-file artifacts/<slug>/issue-status.md
```

Every requirement or scenario change kept after filing, detail or meaningful (*Spec changes*), gets `issue-comment.md` from `spec-author`, which the orchestrator posts with no further approval:

```sh
gh issue comment <n> --body-file artifacts/<slug>/issue-comment.md
```

`issue-body.sh` exit 3 (`body=refused stage-ref=…`): the visible text names a stage or an agent; resume `spec-author` to reword `issue.md`, never edit the body by hand.

The PR link is never added by hand: the PR's `Closes #<n>` line makes GitHub cross-link issue and PR.

### Implement, stage by stage

#### Every stage

- A fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards and `lane=fast|full`; it reads its own rules (`.claude/agents/spec-implementer.md`, `AGENTS.md`).
- **At most 3 code stages per implementer** (`s0` does not count). The next stage gets a fresh one, which starts from its cards and plan sections, never from its predecessor's context. Fixes for an earlier stage (gate 3 findings, PR feedback, a red gauntlet) go to the current implementer, briefed with that stage's card.
- Not done until the plan's Verification method has run and its outcome is reported. Waiving it requires asking.
- **The draft PR body is the board, rendered** by `pr-body.sh <slug> draft` — regenerated and pushed with `gh pr edit <pr> --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)` on **every card move** once the PR exists, with a plain `## Architecture` section and each stage's verification as sub-items under its line; stage ids only in HTML comments.
- **The first push opens the draft PR** from that file; orchestrator records `pr` on the parent card, logs `pr=draft`. Every later push updates it; the user reviews there (*PR feedback*).

```sh
sh .claude/scripts/pr-body.sh <slug> draft > artifacts/<slug>/pr-draft.md
gh pr create --draft --title "$(head -1 artifacts/<slug>/pr-draft.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)
```

#### Fast lane

Per stage:

1. Green: the implementer commits, card → `inreview/`, answers `status=committed stage=s<n> sha=<sha> delta=<n>` — no per-stage code review, no approval stop.
2. Orchestrator verifies the reported sha (below), runs `gauntlet.sh`, pushes, logs `gauntlet=pass sha=<sha>` on the parent card. `delta>0` → fresh `spec-reviewer`, scope `delta s<n>` (*Spec changes*): `count>0` → resume the implementer with the `review.md` path, fixes committed on green, gauntlet, push, follow-up pass by the same reviewer, rounds per *Verify before an approval stop*; a `Needs user` item → a *Delta* stop: approving will apply the answers (keep · revert · split) · continue at the next stage; answer `<1: keep | revert | split · …> · stop`. Then card → `done`, the next stage (*Every stage*). Red gauntlet → card → `inprogress/`, resume the implementer with the gauntlet line.
3. Last stage `done` → *PR feedback*, then *Gate 3 — review*: its branch review is the only code review of a fast-lane run.

Fixes in the fast lane (gate 3 findings, PR feedback, red gauntlet) are committed on green too: the implementer answers `status=committed`, the orchestrator runs `gauntlet.sh` and pushes before any re-review — a review diffs commits, never a dirty worktree.

#### Full lane

Per stage:

1. Green: card → `inreview`, `status=inreview stage=s<n>`. Orchestrator runs `gauntlet.sh` and the per-stage review below.
2. Both clean → *Stage stop* rules decide: a rule fires → user approval stop; none fires → auto-continue (no stop). Either way the same implementer (resumed) commits, answers `status=committed`.
3. Orchestrator verifies the reported sha (below), pushes the worktree (push is never the implementer's), runs `gauntlet.sh` on the pushed sha (`cached` when the commit is the reviewed tree), logs `gauntlet=pass sha=<sha>` on the parent card, card → `done`.

Sha check (both lanes), before any push: `git -C <worktree> cat-file -e <sha>^{commit}`, `git -C <worktree> branch --contains <sha>` lists the run's branch, and the card's last `commit=` line equals the reported sha. Any mismatch → *Question and blocked stage* (`Blocked · <slug> · s<n> · sha mismatch`), no push.

#### Per-stage review

Full lane. Every green stage, before its approval stop:
- Fresh `spec-reviewer`. Base ref = previous stage's commit (branch point for the first stage); scope = that stage id; `spec-reviewer.md`'s `## Axes, reported separately` and `## Spec changes` on one stage's diff.
- `clean`, or `findings` with `count=0` → approval stop, forward the gauntlet line; minors per *Verify before an approval stop*.
- `count>0` → card → `inprogress/`, resume implementer with `review.md` path and stage id, re-run `gauntlet.sh`, follow-up pass by the same reviewer; rounds per *Verify before an approval stop*. An unreviewed stage is never committed.
- **The last code stage**: review in scope `branch` instead — it is gate 3's review, and the stage's approval stop is gate 3's stop.

#### Stage stop

A green, clean-reviewed stage stops for approval only when one of these holds; otherwise it auto-continues. Checked in this order, the first hit is the stop's reason:

1. the verdict has a `Needs user` item — a meaningful spec change or an architecture deviation included
2. the stage's plan Verification goes beyond unit tests (demo TUI, real CSMS, device)
3. *PR feedback* fetched `threads>0`
4. it is the last code stage — its stop is gate 3's
5. the plan marks it `stop` (`plan.summary.md` `Stop` column, settled at gate 2)
6. its diff exceeds 400 changed lines or 8 files (`git diff --stat <base>...HEAD`)
7. checkpoint: three stages auto-continued since the last stop

`s0` never stops.

**Auto-continue** is one line in chat, no answer awaited, and the draft PR update:

```
Landed s2 · <slug> · review clean · gauntlet pass <cov> · pushed <sha> · spec detail: <IDs | none> · next: s3 (auto-continue — say stop to hold)
```

**Stop:** the heading carries the rule's reason (`· stop: demo-TUI verification`); approving will commit s<n> · push, open or update the draft PR · spawn the next stage. Answer: `approve · change <what> · stop`.

Log `auto=<rule-free>` or `stop=<rule n>` on the stage card. A user message while a stage is running is read before the next auto-continue; `stop` holds at the next green.

#### Question and blocked stage

Either lane.
- `status=question` → *Relayed question*, headed with the stage id; the answers go to the same implementer.
- `status=blocked` → one stop, the card stays where it is:

```
Blocked · <slug> · s<n> · <reason>
Answer: answer <text> · re-plan · stop
```

- `answer` → relayed to the same implementer. `re-plan` → *Re-plan*, the reason verbatim. `stop` → the run halts; the board stays for *Resume an interrupted run*.
- `re-plan: <why>` at any stage stop, or in reply to an auto-continue line → *Re-plan* with that reason.

#### Re-plan

Rewrites every stage without a commit on the branch, the current one included; a committed stage is never replanned.
1. `from` = the lowest stage id whose card has no `commit=`. Log `replan from=s<n>` on the parent card, snapshot `cp artifacts/<slug>/plan.md artifacts/<slug>/plan.prev.md` (no `plan.md`, a rebuilt board → no snapshot). The current implementer is dropped, never resumed; its uncommitted work stays in the worktree.
2. Fresh `spec-planner` (*Gate 2* input, plus `mode=replan from=s<n>`, the reason verbatim, the worktree path). `question`, `arch-gap` and `split` as at gate 2.
3. `ready` → fresh `spec-reviewer`, scope `plan from s<n>`; rounds per *Verify before an approval stop*.
4. **Stop**, either lane: `show-file.sh` on `plan.summary.md` (plus `architecture.summary.md` when its rev moved since the `arch` approval — an arch-gap during the re-plan — and `spec-diff.md` when its rev moved, both with `PROMPT_SINCE_REV`; `review.verdict.md` with a `Needs user` item), *Approval prompt* headed `Re-plan · <slug> · from s<n>`. Approving will: replace the cards of `s<n>` onward · update the draft PR · spawn a fresh implementer at `s<n>`. Answer: `approve [stop: s4,s5] · switch lane · change <what> · stop`.
5. On approval: for every card-to-replace's `spec-delta … added` line, re-log it on the parent card with `card-log.sh <slug> spec-delta <ID> added origin=<its origin> -- retired`, so their IDs stay retired, delete those cards, create the new ones as *Gate 2* step 4, log `replan=approved`, render the draft PR body, spawn the implementer (*Every stage*); its brief says the worktree holds the dropped implementer's uncommitted work.

### PR feedback

The user reviews on the draft PR, as inline comments on the diff. Once `pr` is on the parent card, **before every approval stop** (per-stage, gate 3, gate 4) and whenever the user says to check the PR:

```sh
bash .claude/scripts/pr-feedback.sh fetch <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md
```

One status line: `pr=<n> state=… draft=… threads=<count> file=…`.

- `threads=0` → proceed to the approval stop.
- `threads>0` → this is a review pass, handled like `review.md` findings with `count>0`: log `feedback=<count>` on the parent card, implicated stage cards → `inprogress/` (a thread's `path:` names the file; the stage whose `files` list carries it owns it — unknown → the last stage), resume the implementer with the `pr-feedback.md` path. It fixes, fills each thread's `reply:` block, and answers its lane's status (*Fast lane*: `committed`; else `inreview`); then the usual review path on the touched stages (fast lane: gate 3's branch review). After the push:

```sh
bash .claude/scripts/pr-feedback.sh reply <pr> .claude/tasks/artifacts/<slug>/pr-feedback.md s<n> <sha>
```

posts every filled reply block (`<stage> <sha>`: the stage and the pushed head), resolving `fixed` threads and leaving `declined` ones open for the user. A comment asking for behavior the spec doesn't cover is a spec change the implementer makes (*Spec changes*); the review classifies it. `status=blocked` (a comment is a question, or ambiguous) → relay `reason=` to the user as a question, never answer on the PR for them.

A thread the user resolves themselves, or one whose `reply:` they answer on the PR, drops out of the next fetch. Gate 4 does not run while `threads>0`.

### Spec changes

The spec is drafted best effort and settles as the run learns:

| Phase | Who changes it |
|---|---|
| architecture | `spec-architect`, in `spec-diff.md`, local IDs |
| gate 2 | `spec-planner`, in `spec-diff.md`, then fixes IDs |
| after `s0` | `spec-implementer`, in the branch's `docs/specs/`, inside the stage commit |

Before `s0` the agents ask the user through their question batches; what they decided alone shows in the next stop's `Confirm:` block.

**After `s0`.** The implementer logs `spec-delta` on its card and reports `delta=<n>`. The stage's reviewer (full lane: the per-stage review; fast lane: a fresh `spec-reviewer` in scope `delta s<n>` whenever `delta>0`) classifies each change (`spec-reviewer.md` `## Spec changes`):
- **detail** → listed on the verdict and the auto-continue line; no decision. Kept: `spec-author` writes `issue-comment.md`, posted (*Tracking issue*).
- **meaningful** → a `Needs user` item: keep, revert, or split. Stage-stop rule 1 fires. The answer goes to the implementer.
  - **keep**, issue filed → resume `spec-author` for `issue-comment.md`, post it (*Tracking issue*), log `spec-keep=<IDs>` on the parent card. A kept architecture deviation → also a fresh `spec-architect` records it in `architecture.md` and its summary, no stop.
  - **revert** → the implementer reworks to the approved text, gauntlet, follow-up pass by the same reviewer.
  - **split** → below; the implementer narrows the entry.

**Split** — a change needed only for something outside the goal; the user's choice at a question or a `Needs user` item. Before `s0` the raising agent (architect, planner) has already narrowed the entry when it answers `status=split reason=…`; after `s0` the reviewer only raised it, and the implementer narrows it last (below). Orchestrator resumes `spec-author` with the reason verbatim (after `s0`: the verdict item and the `review.md` path); it writes `split-issue.md`. Orchestrator files it (`gh issue create`, as in *Tracking issue*) with `Split from #<issue>` appended as its last line — at once if the tracking issue is filed, else right after gate 2 approval step 1 — appends it to the parent card's `split:`, updates the issue's status line (*Tracking issue*), posts one notice line `Split · <slug> · <IDs> · #<m> filed`, then resumes the architect or planner that raised it — after `s0`, the implementer with `split <ID>: narrow it`, gauntlet, the usual review. The split issue's body is `split-issue.md` with the status line `Split from #<issue> · no spec yet` prepended. The new issue is its own later run.

**Fold in** — the user's explicit override of a split: the raising agent rewrites the `Goal:` line to the widened scope (after `s0`: it is a meaningful change, kept).

Gate 3's reviewer checks every branch spec change was classified; gate 4's sync makes `spec-diff.md` the final spec, never one the orchestrator composes.

### Gate 3 — review

Before proposing a PR, whole-branch pass — cross-stage bugs, spec drift across stages, scope creep:
- Fresh `spec-reviewer`, scope `branch`. Give it base ref, artifact dir, worktree path, all stage ids; it reads its own rules (`AGENTS.md`, `spec-reviewer.md`'s `## Axes, reported separately`). Already run as the last code stage's review (*Per-stage review*) and no code changed since → skip to gate 4.
- Orchestrator runs `gauntlet.sh` on the branch, forwards both status lines.
- `count>0` → as *Per-stage review*, the same implementer, then a fresh branch review — never a follow-up pass here. Findings needing a user decision are relayed as questions, never fixed unasked.
- Log `review=clean` or `review=<n>-resolved` on the parent card once the branch review is clean (the PR status line reads it). Minors (*Verify before an approval stop*) are resolved before `spec-author` writes `pr.md`; gauntlet re-run, `gauntlet=pass sha=<sha>` logged on the parent card.
- **Fast lane:** no stop here — clean goes straight to gate 4, whose stop shows the verdict.
- **Stop (full lane):** approving will hand the branch to `spec-author` for the PR body (gate 4). Answer: `approve · change <what> · stop`.
- **Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage id, and rewrites `review.verdict.md`. Parent card → `inreview/` when review starts.

### Gate 4 — pull request

- Gauntlet + gate 3 clean, *PR feedback* at `threads=0`.
- **ID collisions** — another run may have landed the same fixed IDs meanwhile: `git -C <worktree> fetch -q origin main && sh .claude/scripts/id-collisions.sh <worktree>`. `ids=collide <IDs>` → resume the implementer to renumber them on the branch (spec, test citations, `spec-delta` log), gauntlet, push, check again.
- `spec-author`, given the worktree path and the board root, syncs `spec-diff.md` from the branch's `docs/specs/`, writes `artifacts/<slug>/pr.md` in **full form** and one `follow-up-<k>.md` per thing the run noticed outside its goal (implementer cards' `follow-up` lines, `review.md`'s `follow-up — ` lines; answers `follow-ups=<n>`) (content rules in its file; `.github/PULL_REQUEST_TEMPLATE.md` carries the same sections for human PRs). Orchestrator assembles both bodies:

```sh
sh .claude/scripts/pr-body.sh <slug> full > artifacts/<slug>/pr-full.md   # exit 3 (body=refused stage-ref=…): resume spec-author to reword pr.md
```

- **One stop**: `show-file.sh` on `pr.md` and every `follow-up-<k>.md` (fast lane: plus `review.verdict.md`, per *Agent hand-off*); approving will push · replace the draft's title and body · mark #<pr> ready · file the <n> follow-up issues · wait for CI · clean up. Answer: `approve [follow-ups: 1,3 | none] · manual run first · change <what> · stop` — no list files every follow-up — the user may want a manual run; don't pre-empt it. Approved → push, replace the draft's title and body, mark ready, log `pr=ready`:

```sh
gh pr edit <pr> --title "$(head -1 artifacts/<slug>/pr-full.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-full.md)
gh pr ready <pr>
```

- No draft exists (run predates this rule, or resumed from a board that lacks `pr`): `gh pr create` with the same title and body file instead of `edit`+`ready`.
- **Follow-ups**, right after `pr=ready`: each one the answer kept is filed as in *Tracking issue* (title = line 1, body = the rest) with `Found in #<issue>` appended as its last line; log `follow-ups=[<n>, …]` on the parent card. A dropped one is never filed.
- **CI** — wait for the PR's checks, one status line in context:

```sh
gh pr checks <pr> --watch > artifacts/<slug>/ci.log 2>&1; echo "ci=$?"
```

  - `ci=0` → status line `done` (*Tracking issue*), `sh .claude/scripts/metrics.sh <slug>`, then *Clean up*.
  - otherwise → `bash .claude/scripts/failed-workflow.sh <branch>`, never raw `gh run view`; implicated stage card → `inprogress/`, resume or spawn the implementer to fix, gauntlet, push, wait again. No stop unless a fix needs a user decision.

### Clean up

The run's last step, right after CI is green and `metrics.sh` has run — no approval stop: the issue and the PR hold everything that outlives the run, `metrics.log` its cost. The human reviews and merges on GitHub, outside the workflow.

```sh
sh .claude/scripts/run-cleanup.sh <slug>
```

It refuses (exit 1, one line) while the worktree has uncommitted changes or commits its upstream lacks; else it removes the worktree, deletes the local branch (the pushed one stays for the PR) and moves every card and `artifacts/<slug>/` into `.claude/tasks/archive/<slug>/` (never pruned; a second run of the same slug gets a dated name), printing `cleanup=done archive=<path>`. One notice line: `Done · <slug> · PR #<pr> ready · CI green · cleaned up · archived`.

### Resume an interrupted run

**Step 0.** `git -C <main checkout> pull --ff-only` before reading anything.

**No card, PR exists** (the user names the issue and its draft PR): `sh .claude/scripts/run-rebuild.sh <issue> <pr>`. `rebuild=done … next=s<n>` → the reconcile table below, then *Re-plan* from `next=` with a fresh planner (`plan.md` is never rebuilt; `next=none` → *Gate 3*). `rebuild=refused reason=no-stages` → the PR is ready: CI check, then *Clean up*. No PR → gate 1 with "take issue #n". Any other refusal → report it.

Cards outside `open/`+`done/`, no agent running = session died mid-run. Resume triggered by the user or `/spec-feature`, never automatic.

`kind: epic` with `issue` set → the epic stop was approved: continue filing at the first slice not yet in `slices:` (step 5.2 adopts one filed before the crash), never re-file one. Otherwise: no worktree on the card → died during gate 1, architecture or gate 2, nothing on disk to reconcile — resume the conversation from `spec-diff.md`/`architecture.md`/`plan.md`'s last state (`arch` set → gate 2). `issue` set but no worktree → gate 2 was approved: continue at its step 2, never re-file. Past gate 2 → table below. **Any resumed implementation spawns a fresh agent**, its brief naming the card's `lane`.

**Reconcile before acting.** Card = intent, git = fact:

| Card claims | Check | Disagreement means |
|---|---|---|
| worktree | `git worktree list` | card stale |
| branch | `git rev-parse` | stage never started |
| `commit=<sha>` | sha exists, on that branch | commit never landed |
| `issue=<n>` | `issue-view.sh <n>` — open; a comment per `spec-keep=` on the parent card | missing comment: post `issue-comment.md` (*Tracking issue*) |
| `pr=<n>` | `pr-feedback.sh fetch <n> …` — its status line's `state=`/`draft=` | `pr=ready` logged, or closed/merged: the run is over — CI check, then *Clean up*; missing: draft never opened, the next push opens it |
| `gauntlet=pass` | `gauntlet.sh` at that sha | card overstated state |
| stage `done` | `git branch --contains <commit>` includes the feature branch | never landed; downstream plans a lie |
| PR `## Stages` | the PR body vs `pr-body.sh <slug> draft` | body behind the board: a move never rendered, `gh pr edit` with the fresh render |

Report differences first. Agree → resume. Disagree → stop and report.

A `replan from=` line with no later `replan=approved` on the parent card → the re-plan is pending: rerun *Re-plan* from step 2 with a fresh planner; `plan.prev.md` is already the snapshot.

Clean reconcile → resume only no-approval work (respawn the implementer for approved stages, CI wait and *Clean up* after `pr=ready`); halt at the first gate needing the user. Recorded `gate1`/`arch`/`gate2` approvals stay valid — the fast lane's pre-authorization too: `arch` set, `lane: fast`, no `gate2` → plan, then gate 2's *On approval* without a stop. A resumed run spawns every agent fresh (the architect, the author and the planner re-read their files).
