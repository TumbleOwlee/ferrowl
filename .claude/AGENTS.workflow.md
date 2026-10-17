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
- **A follow-up in the same scope resumes the agent that did the first pass** (`SendMessage`) — reviewer, planner, architect, author — never a respawn; fresh only when that agent is gone (a resumed session) and where a section says so: every gate 3 `branch` review, a re-plan's planner, each new implementer.
- **The run ends when the PR is ready and CI is green** — then *Clean up* removes the worktree and local branch and archives the board. Merge happens outside the workflow.
- **Every state change moves a task-board card.** State living only in conversation dies with the session.
- **Every stop is logged** — `prompt-render.sh` logs `stop kind=<question|approval|stage|blocked>` on the parent card as it renders the stop (`approval`: gate, architecture, epic, delta and re-plan stops). `metrics.sh` counts them at the end of the run. A workflow change that adds stops or agents names its expected effect on `metrics.log` in its PR; one that changes a card move, a stop rule or a prompt updates the replayed runs (`.claude/scripts/tests/replay.sh`) in the same PR.

### Lane — fast or full

The lane sets how many approval stops the run makes. An epic run (*Epic*) has no lane: it stops at its brief and at the epic stop; each slice picks its own lane. Proposed by size when the first agent output of the run is ready; the user settles it at the run's first approval stop, recommended lane first.

Recommended default, by size:
- **fast** — ≤ 3 `R`/`E` entries in the brief, or `no-diff`; non-behavior change: `no-arch`, or ≤ 3 new or changed components (architecture `count=`). At most 1 entry, or `no-diff`: no architecture step (`arch: skipped`), the planner starts from the brief.
- **full** — anything bigger.

| | fast | full |
|---|---|---|
| Brief (gate 1) | no stop of its own — approved at the architecture stop, or at gate 2's stop without one | own stop, settles the lane |
| Architecture | one stop: brief + architecture + lane; approving pre-authorizes gate 2's *On approval*; no architecture step at ≤ 1 entry | own stop |
| Gate 2 | a notice unless `next-step.sh` stops it (*Gate 2*); no architecture step: one stop, brief + plan + issue | plan summary + issue draft |
| Stages | *Fast lane* | *Full lane*, stops per *Stage stop* |
| End | one stop, *Gate 4 — pull request*, showing the branch verdict and any `Needs user` item it left | *Gate 4 — pull request* stop with the branch verdict; a separate *Gate 3 — review* stop only for a `Needs user` item |

No architecture step and the planner answers `arch-gap`: spawn `spec-architect` fresh with the brief and the reason; its `ready` → the architecture stop (approves the brief), then resume the planner.

Recommended full, user picks fast at the brief stop: the architecture stop is fast's. The fast default for a plan is at most 2 stages besides `s0` (*Gate 2*). The user may switch lanes at any later stop; a lane never switches silently.

### Verify before an approval stop

By running, never by reading:
- **Gauntlet** — `sh .claude/scripts/gauntlet.sh <worktree> .claude/tasks/artifacts/<slug>` in the agent's worktree: one status line in context, full output in `artifacts/<slug>/gauntlet.log`. The full set runs once per tree: a pass is cached by the worktree's tree, so the same tree again (committed, pushed) answers `gauntlet=pass … cached` at once. The implementer runs only fmt, clippy and its crates' tests; the full set is this one. `GAUNTLET_FORCE=1` reruns a cached tree.
- **A fresh `spec-reviewer` per scope** — never the planner or implementer: one sharing the author's context reproduces its blind spots. A follow-up pass in the same scope resumes that reviewer (*Principles — implementation*) with the `review.md` path and `follow-up` (`spec-reviewer.md` `## Follow-up pass`). Plan scope: before resuming the planner with findings, the orchestrator snapshots `cp artifacts/<slug>/plan.md artifacts/<slug>/plan.prev.md`, so the follow-up diffs the revision itself instead of trusting the planner's account.
- **Minors never wait for the user.** The verdict names where they go: `Minors (folded into s<n+1>)` when a stage follows, `Minors (fixed before commit)` when none does — a last stage, a branch review. Folded: the orchestrator logs each as `minor=<verdict line>` on the next stage's card and repeats them in the implementer's brief. Fixed before commit: the implementer is resumed with them at once, fixes, gauntlet, no re-review, then the stage's commit (fast lane: commit on green). Only `count>0` (blockers + majors) loops, and at most **two** fix-and-review rounds per scope: findings still open after the second round go to the user as `Needs user` items on the verdict.
- **Overturned findings are logged.** The user picks another option than the verdict recommends, or calls a finding wrong (a `change`, a PR reply): log `overturned scope=<scope> count=<n> -- <the finding, ≤ 8 words>` on the parent card. `metrics.sh` sums them per run — the evidence for tuning `spec-reviewer.md`'s axes, never a reason to drop a finding mid-run.

### Task board

`.claude/tasks/` holds the cards (*Cards*), each run's files (*Artifacts*), `metrics.log` (one line per finished run, written by `run-cleanup.sh`) and `archive/<slug>/` (a finished run's cards and artifacts, `run-cleanup.sh`); neither of the last two is ever cleaned up.

#### Artifacts

```
.claude/tasks/artifacts/<slug>/
  spec-diff.md       gate 1 brief, then the run's spec until s0 — local IDs until gate 2; synced from the branch at gate 4
  decisions.md       every decision of the run, one line each, appended by every agent — the one source of Confirm: and the PR's ## Decisions
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
  request.md         the request issue's original body, stored once (gate 1 or reuse)
  issue-comment.md   changelog comment of the latest user-approved spec change after filing
  split-issue.md     an issue split off (*Split*)
  follow-up-<k>.md   gate 4: an issue for something the run noticed outside its goal
  pr.md              gate 4 PR title + body (*Gate 4 — pull request*)
  pr-feedback.md     unresolved PR review threads (*PR feedback*)
```

`artifacts/<slug>` everywhere in this file and the agents' files is shorthand for `.claude/tasks/artifacts/<slug>` in the **main checkout**: every spawn brief passes its absolute path (an agent in a worktree would resolve a relative one inside the worktree), every orchestrator command runs from the main checkout's root.

#### Cards

Directory a card sits in **is** its state: `.claude/tasks/{open,inprogress,inreview,done}/`. Every move, the implementer's included, is `sh .claude/scripts/card-move.sh <card-id> <state>`: it refuses a transition the table below forbids (a stage to `done` without a `commit=` the parent's `gauntlet=pass` verified, a stage started before its `blocked-by`, the parent before `gate2`) and logs `moved=<state>`; `hook-guard-shell.sh` denies a raw `mv` of a card. "card → `<state>`" in this file means that call.

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

Log line = `<ISO minute> <event> [key=value …]`; the implementer's own file carries the stage-event vocabulary. Every line on any card is written by `sh .claude/scripts/card-log.sh <card-id> <event> [k=v …] [-- <text>]` (vocabulary in the script; `hook-guard-shell.sh` denies a raw `>>`/`tee -a` onto a card); "log <event>" in this file means that call. A free-text remark on any card (a rename, a correction) is `note -- <text>`, never starting with an event's name — `card-log.sh` refuses it with `reason=typed use=<event>`. A mid-run addition to a stage's `files` is logged on its card as `files+=<path>[, <path>] user=approved reason=…`, a minor handed to another stage as `minor-moved=<verdict line>` (on the stage that gave it up). `card-log.sh` stamps every line with `date -u +%FT%H:%M` — UTC on every card, so a resumed session can order events across cards.

Parent frontmatter: `kind: epic|slice` (absent on a plain run), `epic` (slice run: the epic's issue number), `slices: [<n>, …]` (epic run: the child issues, in slice order), `issue`, `pr` (number, set when the draft opens), `branch`, `lane: fast|full` (set at the first approval stop), `gate1` approval date, `with-arch` (fast lane, until the architecture stop), `with-plan` (fast lane without an architecture step, until gate 2's stop) or `skipped`, `arch` approval date, `no-arch` or `skipped` (fast lane without an architecture step), `gate2` approval date, `split: [<n>, …]` (issues split off), `upstream: [<repo>#<n>, …]`, `artifacts`. `card-field.sh <slug> <field>` reads one. Never the goal or normative text — issue holds the goal, `artifacts/` the spec.

| Card | `open` | `inprogress` | `inreview` | `done` |
|---|---|---|---|---|
| stage | created from plan | agent took it | green; under review (fast lane: already committed) | committed, pushed and orchestrator-verified; full lane also reviewed and approved or auto-continued |
| parent | gate 1, architecture or gate 2 pending | implementing | gate 3/4, until *Clean up* archives it; `inprogress` again for a re-plan asked at gate 4 | — never: the run ends in *Clean up* |

Rules:
- **No agent writes its own `done`** — the orchestrator's move (table above).
- **Runnable** = every `blocked-by` id in `done/`.
- No `blocked/` directory — blocking derives from `blocked-by`.
- Reconciling cards with git: *Resume an interrupted run*. Cards end in *Clean up*.

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
| spec-implementer | `inreview` (`delta=`) · `committed` (`sha=`, `delta=`) — `delta=` counts spec changes · `question` (`count=` decisions) · `blocked` (`reason=`) | stage card (`stage=`), `question.md` | none — the card is the summary |
| spec-reviewer | `clean` · `findings` (`count=` blockers+majors, `user=` `Needs user` items, `stage=` list) · `blocked` | `artifacts/<slug>/review.md` | `artifacts/<slug>/review.verdict.md` |

`file` is lossless, for the next agent: as long as it must be. `summary` is for the user: decisions and open findings only, rewritten whole on every pass, capped at 25 lines / 2 KB (`show-file.sh` refuses a larger one and exits 3 — the agent trims it).

- `question` = a batch of decisions in `artifacts/<slug>/question.md` (*Relayed question*).
- An agent returning more than the status line is told to move the rest into its file and answer again.
- A status line naming a file the user must approve (`spec-diff.md`, `pr.md`, a `summary=` path) → the stop's `prompt-render.sh` call (*Approval prompt*): it opens the files outside the context (`show-file.sh`) and quotes only their `prompt-lines.sh` lines. Exit 3 = a summary over its cap: the agent that wrote it trims it, never show the full file instead.

At gate 2, a re-plan, a branch review, gate 4 and CI the status line goes to `sh .claude/scripts/next-step.sh <slug> <plan|replan|branch|pr|ci> <status line>`: it logs the typed event (`plan-review=`, `review=`, `ci=`, `replan=pre-authorized`) and prints `next=`, the command, what to do and the section; its header has the rules.

Reviewer scope tokens: `plan` (gate 2), `plan from s<n>` (*Re-plan*), `stage s<n>`, `delta s<n>` (one stage's spec changes, *Spec changes*), `branch` (gate 3).

### Approval prompt

Every stop is one message rendered by `sh .claude/scripts/prompt-render.sh <kind> <slug> [k=v …]` and relayed verbatim; the orchestrator composes none of it. The script picks the files the stop shows from the cards (`spec-diff.md` while the brief is unapproved or its rev moved since the last approval), opens them in the viewer, quotes their lines in fixed order, adds what approving will do, and logs the stop. Kinds and their arguments: the script's header; each stop's section names its call.

Two shapes, one answer grammar each:
- **Approval** — gate, architecture, epic, stage and re-plan stops. Ends in `Approving will:`, `Options:` (numbered, when the stop has any) and `Answer: approve [<n>[: <value>] …] · change <what> · stop`. `approve 2` takes option 2; `change` goes to the agent that wrote the file; `stop` halts and keeps the board (*Resume an interrupted run*). A `Needs user` item is answered in the same reply by its option letter (`approve · a`; several items: one letter each, in order) and relayed with the approval. `re-plan: <why>` is accepted at any stage stop, at gate 4's stop and in reply to an auto-continue line (*Re-plan*).
- **Question** — an agent's decision batch, a blocked stage, a stage's meaningful spec changes. Numbered decisions, lettered options, `Answer: <1: a · 2: b · …> · other <text>`.

Notices (`auto`, `gate2-notice`, `replan-notice`) are one line, no answer awaited, never logged as a stop.

Approved → `sh .claude/scripts/prompt-render.sh approved <slug>` first: it logs `spec-rev=`/`arch-rev=` for the files the stop showed and `decisions-seen=` for `decisions.md`, so the next stop shows only the files that moved and confirms only the decisions written since.

### Relayed question

An agent's `status=question` names `artifacts/<slug>/question.md` (shape: `spec-author.md` `## Questions`): `prompt-render.sh question <slug> where=<gate or s<n>> agent=<agent>` prints it whole as the prompt, never in the viewer. Answers go back to the agent that asked (*Principles — implementation*) in one message, numbered. An implementer's answered design-grade decision is appended by the implementer to `decisions.md` as `(user) [design]`; a behavioral answer lands as a spec change with its `spec-delta`; the next card move's draft render shows either. A decision marked `Assumes: <n> = <option>` is void when the user answered `<n>` otherwise; the agent asks it again in its next `question.md`, as it does a decision that depended on an answer and waited. A question without a real `Why you:` is sent back: the agent decides it and records `(chosen)`.

### Gate 1 — brief

`spec-author` grills the user and drafts the brief, `spec-diff.md` (its own file holds the grilling rules and the shape); user decides, orchestrator relays. Best effort: scope, scenarios, the user-visible flow and the edge cases the user cares about; everything else lands under `## Deferred` with an owner. Spawn `spec-author` with: user's goal verbatim, affected area(s), `artifacts/<slug>/`, and the issue number when the user named one ("take issue #n") — the request and the *Tracking issue* reuse candidate.

- **Board:** create `open/<slug>.md` + `artifacts/<slug>/` before spawning. Named issue: store its body first, `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md`, and the brief it may already carry, `sh .claude/scripts/issue-request.sh <n> --spec > artifacts/<slug>/issue-spec.md`; delete either file when empty. `sh .claude/scripts/issue-request.sh <n> --epic` prints a number → the issue is a slice: *Slice run* first.
- `status=question` → relay question and answer without comment.
- `status=ready size=epic` → always a stop, whatever the lane would be: `prompt-render.sh gate1 <slug> size=epic reason=<the author's reason>`. Option 1 (epic) → record `kind: epic` and `gate1`; *Epic*. Option 2 or 3 (single run) → as below, with that lane.
- `status=ready` or `status=no-diff` (spec already covers the goal; the file names the violated requirements) → recommend the lane by `count=` (*Lane — fast or full*):
  - **full** recommended → `prompt-render.sh gate1 <slug>`: approval **and lane** in one prompt. Change requested → relay to the same agent. Approved → record `gate1` and `lane` on the parent card.
  - **fast** recommended → no stop: record `lane: fast` and `gate1: with-arch`; at most 1 entry or `no-diff`: `gate1: with-plan` and `arch: skipped`, then *Gate 2*. Else the brief is approved at the architecture stop.
  - Then *Architecture* (skipped: *Gate 2*).
- The agent stays alive for *Tracking issue*, *Spec changes* and gate 4.

### Architecture

`spec-architect` reads the code, grills the user on structure and drafts `architecture.md` (its own file holds the shape); user decides, orchestrator relays. Spawn it with: `artifacts/<slug>/spec-diff.md` path (non-behavior change: the goal verbatim instead), affected area(s), anything the user volunteered at gate 1.

- `status=question` → *Relayed question*.
- `status=split` → *Split*.
- `status=behavior-change` (non-behavior run) → *Principles — implementation*.
- `status=no-arch` → record `arch: no-arch`; no stop; gate 2 (fast lane: its stop approves the brief).
- `status=ready slices=…` → the user chose an epic at the architect's size guard: record `kind: epic`; *Epic* step 3.
- `status=ready` → `prompt-render.sh arch <slug>` (shows `architecture.summary.md`, `architecture.md` with the interface sketches, and the brief when still pending — fast lane — or moved; a non-behavior run's first stop offers the lane). Change requested → relay to the same agent.
  - Approved → record `arch` (fast lane: `gate1` too) on the parent card; *Gate 2*. Fast lane: the approval pre-authorizes gate 2's *On approval* once the plan review is clean; next stop gate 4.
- The agent stays alive until gate 2's approval, for the planner's `arch-gap`; a resumed run spawns a fresh one, which re-reads `architecture.md`.

### Epic

A feature too big for one run: the author answered `size=epic`, or the architect's size guard tripped. The epic run settles the shared core and cuts the feature into slices; it writes no code and has no plan, worktree or PR. Each slice is later a normal run on its own issue.

1. **Brief** — gate 1 as usual; its stop approves `epic`. Or the architect's size guard turned a behavior run into one: record `kind: epic`, drop `lane` from the card, go to step 3; the epic stop approves the brief too when `gate1` is not yet a date (fast lane).
2. **Skeleton and slices** — spawn `spec-architect` with the brief and `mode=epic`. It writes `architecture.md` and its summary for the **skeleton** only (the core abstractions every slice builds on, with their `## Interfaces`) and `slices.md` (shape: its file). Questions relayed as usual; it answers `status=ready … slices=artifacts/<slug>/slices.md`.
3. **Epic issue draft** — `spec-author` drafts `issue.md` (*Tracking issue*: existing issues searched, `reuse` or `new`).
4. **Epic stop** — `prompt-render.sh epic <slug> [issue=<n>]` (`<n>`: the author answered `reuse`); its `Confirm:` names every brief ID no slice or two slices claim. Approved → record `gate1` (when still pending) and `arch`.
5. **On approval, in order** — no further stop:
   1. File or rewrite the epic issue from `issue-body.sh <slug>` (*Tracking issue*); record `issue`.
   2. Per slice, in `slices.md` order: `sh .claude/scripts/issue-body.sh <slug> --slice <n> > artifacts/<slug>/slice-<n>.md`; an open issue already titled exactly as its line 1 (`gh issue list --state open --search "<title> in:title"`) is adopted, never filed twice — else `gh issue create` from it (title = line 1, body = the rest); then append the number to the parent card's `slices:` — a later slice's `Depends on` links it.
   3. Reassemble and edit the epic issue's body — the one exception to *Tracking issue*'s never-rewritten rule: its `## Slices` list now links every child.
   4. *Clean up* — no worktree, no CI; it records the metrics; cards and artifacts go.
   5. One notice line: `Epic · <slug> · #<e> · slices #a, #b, … filed · start with /spec-feature on #a`.

The human closes the epic once its last slice merges.

#### Slice run

`/spec-feature` on a child issue ("take issue #n"), a normal run otherwise.
- **Gate 1 board:** record `kind: slice` and `epic: <e>`; store the slice's `**Part of epic**` line, `sh .claude/scripts/issue-request.sh <n> --slice-line > artifacts/<slug>/slice-line.md` (`issue-body.sh` puts it under the run's title), and the epic as it stands, `issue-request.sh <e> --spec > artifacts/<slug>/epic.md` then `issue-request.sh <e> --comments >> artifacts/<slug>/epic.md`. Name `epic.md`'s path in every spawn brief of the run except the implementer's.
- **Kept skeleton change** — its `issue-comment.md` is posted on the epic too: `gh issue comment <epic> --body-file …`, the card's `epic`.
- **Split** — a slice's split-off issue gets `**Part of epic** #<e> · split from #<slice>` prepended as its first line, and a one-line comment on the epic names it.
- Slices whose `Depends on` don't chain may run at the same time; gate 4's ID collision check covers them.

### Gate 2 — plan and tracking issue

Spawn `spec-planner` with: `artifacts/<slug>/spec-diff.md` path (none for a non-behavior change), `artifacts/<slug>/architecture.md` path (none on `no-arch` and when the architecture step was skipped), `arch=skipped` when it was, affected area(s), anything the user volunteered. Nothing else; the agent explores the repo itself, starting from the architecture's code map. It writes `plan.md` (shape: `spec-planner.md`'s `## Output`) and `plan.summary.md`, edits `spec-diff.md` and assigns its fixed IDs.

Every status line of the gate (planner, plan reviewer, the author's issue answer) → `sh .claude/scripts/next-step.sh <slug> plan …`:
- Review rounds per *Verify before an approval stop*: fresh `spec-reviewer` scope `plan`; any finding, minors included, resumes the planner (`plan.prev.md` first); the same reviewer's follow-up.
- The issue draft: `spec-author` (*Tracking issue*); a non-behavior run spawns it now with the goal verbatim, `plan.summary.md`, `artifacts/<slug>/`. Then the stop or the fast lane's notice.
- `arch-gap` → resume `spec-architect` with the reason verbatim; its `ready` → resume the planner, no stop of its own. `split` → *Split*; `behavior-change` → *Principles — implementation*.
- The plan's `Stop` column (which stages stop for approval beyond the *Stage stop* rules) is settled at this stop: the user may tick or untick stages in the answer.
- **Stop:** `prompt-render.sh gate2 <slug> issue=<new | n>` (`plan.summary.md`, the verdict when findings survived two rounds, the brief when it moved; `issue.md` only as its `Issue:` line). Option 1 adds stages to the `Stop` column, option 2 switches the lane.
- **Notice:** `prompt-render.sh gate2-notice <slug>`, no answer awaited, then *On approval*.

**On approval, in order:**
1. File or rewrite the issue (*Tracking issue*); record `issue` on the parent card. Fast lane without an architecture stop: `gate1` too.
2. Create the worktree: `git worktree add .claude/worktrees/<slug> -b <type>/<slug> main`.
3. Record `gate2` (and `lane`, for a non-behavior change with `no-arch`) on parent card, move parent → `inprogress/`.
4. Create `open/<slug>.s<n>.md` per plan stage (`s0` only when the plan has one), `title`/`files`/`blocked-by` copied from the tree (`title` is display-only — agents find a stage by `--prefix '## Stage s<n>:'`) and `stop: yes|no` from `plan.summary.md`'s `Stop` column as the user settled it — `sh .claude/scripts/extract-section.sh '## Shared' artifacts/<slug>/plan.md` is the one plan read the orchestrator makes, for exactly those three fields. Stage ids match plan ids.
5. Spawn the implementer, its brief naming the lane.

### Tracking issue

Drafted inside gate 2 once the plan review is clean, approved at gate 2's stop (fast lane: pre-authorized at the architecture stop), filed on approval.

Orchestrator searches existing issues (`gh issue list --state all --search …`) for the same goal and passes candidate numbers — plus the one the user named at gate 1 — to `spec-author`, which writes `artifacts/<slug>/issue.md` (title + goal; shape: its file) and answers `status=reuse issue=<n> file=…` or `status=new file=…` in one turn. Reuse, never duplicate. On reuse the orchestrator stores the issue's original body once (unless gate 1 already did) with `sh .claude/scripts/issue-request.sh <n> > artifacts/<slug>/request.md` — the body as filed, or, when that body is one this workflow assembled (it carries `## Scenarios`, `## Spec (rev` or an `Original request` block), the text inside its `<details>` block.

Filing:

```sh
sh .claude/scripts/issue-body.sh <slug> > artifacts/<slug>/issue-body.md
# new
gh issue create --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
# reuse
gh issue edit <n> --title "$(head -1 artifacts/<slug>/issue-body.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/issue-body.md)
```

**After filing, the body is never rewritten** — it carries no status: the draft PR, linked to the issue, shows the progress. One exception: an epic's body is edited once, at *Epic* step 5.3, to link its slice issues.

Every requirement or scenario change kept after filing, detail or meaningful (*Spec changes*), gets `issue-comment.md` from `spec-author`, which the orchestrator posts with no further approval:

```sh
gh issue comment <n> --body-file artifacts/<slug>/issue-comment.md
```

`issue-body.sh` exit 3 (`body=refused stage-ref=…`): the visible text names a stage or an agent; resume `spec-author` to reword `issue.md`, never edit the body by hand.

The PR link is never added by hand: the PR's `Closes #<n>` line makes GitHub cross-link issue and PR.

### Implement, stage by stage

#### Every stage

- A fresh `spec-implementer`, spawned with worktree path, plan path, its stage cards and `lane=fast|full`.
- **At most 3 code stages per implementer** (`s0` does not count). The next stage gets a fresh one, which starts from its cards and plan sections, never from its predecessor's context. Fixes for an earlier stage (gate 3 findings, PR feedback, a red gauntlet) go to the current implementer, briefed with that stage's card.
- Not done until the plan's Verification method has run and its outcome is reported. Waiving it requires asking.
- **The draft PR body is the board, rendered** by `pr-body.sh <slug> draft` — regenerated and pushed with `gh pr edit <pr> --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)` on **every card move** once the PR exists. It is the living PR: status line, `## Why`, `## Scenarios`, `## Stages` (each stage's verification as sub-items under its line, stage ids only in HTML comments), `## Review here`, then the sections the gate 4 body shares: `## Requirements`, `## Spec changes since filing`, `## Architecture` (the current `architecture.md` rev) and `## Decisions` (`decisions.md`'s design lines and its chosen spec lines not already under spec changes).
- `## Spec changes since filing` in the draft = the cards' `spec-delta` lines whose `**<ID>**` the committed `git diff main...<branch> -- docs/specs` touches, last line per ID; omitted without a `branch` or when git fails. IDs added after filing show only under spec changes until gate 4.
- `pr-body.sh <slug> draft` exit 3 (`body=refused stage-ref=<token>`): the token sits in a card's `spec-delta` why or a `decisions.md` line; resume the agent that wrote it to re-log the `spec-delta` (last line per ID wins) or reword its line; never edit the body by hand; the PR keeps its previous body until then.
- **The first push opens the draft PR** from that file; orchestrator records `pr` on the parent card, logs `pr=draft`. Every later push updates it; the user reviews there (*PR feedback*).

```sh
sh .claude/scripts/pr-body.sh <slug> draft > artifacts/<slug>/pr-draft.md
gh pr create --draft --title "$(head -1 artifacts/<slug>/pr-draft.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-draft.md)
```

#### Fast lane

Per stage:

1. Green: card → `inreview/`, the implementer commits (`card-log.sh` refuses `commit=` from `inprogress/`), answers `status=committed stage=s<n> sha=<sha> delta=<n>` — no per-stage code review, no approval stop.
2. Orchestrator verifies the reported sha (below), runs `gauntlet.sh`, pushes, logs `gauntlet=pass sha=<sha>` on the parent card. `delta>0` → fresh `spec-reviewer`, scope `delta s<n>` (*Spec changes*): `count>0` → resume the implementer with the `review.md` path, fixes committed on green, gauntlet, push, follow-up pass by the same reviewer, rounds per *Verify before an approval stop*; a `Needs user` item → the Delta stop below. Then card → `done`, the next stage (*Every stage*). Red gauntlet → card → `inprogress/`, resume the implementer with the gauntlet line.
3. Last stage `done` → *PR feedback*, then *Gate 3 — review*: its branch review is the only code review of a fast-lane run.

**Stop (delta):** `prompt-render.sh delta <slug> stage=s<n>` — the verdict's `Needs user` items as a question batch; the answers (keep · revert · split) go to the implementer, then the next stage.

Fixes in the fast lane (gate 3 findings, PR feedback, red gauntlet) are committed on green too, card → `inreview/` first: the implementer answers `status=committed`, the orchestrator runs `gauntlet.sh` and pushes before any re-review — a review diffs commits, never a dirty worktree.

#### Full lane

Per stage:

1. Green: card → `inreview`, `status=inreview stage=s<n>`. Orchestrator runs `gauntlet.sh` and the per-stage review below.
2. Both clean → *Stage stop* rules decide: a rule fires → user approval stop; none fires → auto-continue (no stop). Either way the same implementer (resumed) commits, answers `status=committed`.
3. Orchestrator verifies the reported sha (below), pushes the worktree (push is never the implementer's), runs `gauntlet.sh` on the pushed sha (`cached` when the commit is the reviewed tree), logs `gauntlet=pass sha=<sha>` on the parent card, card → `done`.

Sha check (both lanes), before any push: `sh .claude/scripts/sha-check.sh <slug> s<n> <sha>` (the commit exists, is on the run's branch and is the card's last `commit=`). `sha=mismatch` → *Question and blocked stage* with `reason=sha mismatch <reason>`, no push.

#### Per-stage review

Full lane. Every green stage, before its approval stop:
- Fresh `spec-reviewer`. Base ref = previous stage's commit (branch point for the first stage); scope = that stage id; `spec-reviewer.md`'s `## Axes, reported separately` and `## Spec changes` on one stage's diff.
- `clean`, or `findings` with `count=0` → approval stop, forward the gauntlet line; minors per *Verify before an approval stop*.
- `count>0` → card → `inprogress/`, resume implementer with `review.md` path and stage id, re-run `gauntlet.sh`, follow-up pass by the same reviewer; rounds per *Verify before an approval stop*. An unreviewed stage is never committed.
- **The last code stage**: review in scope `branch` — it is gate 3's review; its status line → `next-step.sh <slug> branch …` (it answers `stage-stop` or gate 3's stop).

#### Stage stop

A green, clean-reviewed stage: `sh .claude/scripts/stage-stop.sh <slug> s<n> threads=<n>` (`threads=` from *PR feedback*'s fetch) decides and logs it on the card. Its rules, first hit wins: a `Needs user` item, Verification beyond unit tests, open PR threads, a plan-marked stage, a diff over 400 lines or 8 files, three auto-continues since the last stop; `s0` never stops.
- The last code stage: `auto` → commit, push, gate 4; `stop=1` → `prompt-render.sh gate3 <slug>`; any other `stop=` → the `stage` stop; approval commits it, then gate 4.
- `auto` → the commit and push of *Full lane* steps 2–3, then `prompt-render.sh auto <slug> stage=s<n>`: one line naming the pushed sha, no answer awaited, then the draft PR update. A user message while a stage is running is read before the next auto-continue; `stop` holds at the next green.
- `stop=<rule> reason=<word> [detail=…]` → `prompt-render.sh stage <slug> stage=s<n> reason=<word> [detail=…]`.

#### Question and blocked stage

Either lane.
- `status=question` → *Relayed question*, headed with the stage id; the answers go to the same implementer.
- `status=blocked` → one stop, the card stays where it is: `prompt-render.sh blocked <slug> stage=s<n> reason=<reason>`. `1: a <text>` → relayed to the same implementer; `1: b` → *Re-plan*, the reason verbatim; `1: c` → the run halts, the board stays for *Resume an interrupted run*.

#### Re-plan

Rewrites every stage without a commit on the branch, the current one included; a committed stage is never replanned.
1. `from` = the lowest stage id whose card has no `commit=`. Log `replan from=s<n>` on the parent card (asked at gate 4: `from` = next free stage number, parent → `inprogress`), snapshot `cp artifacts/<slug>/plan.md artifacts/<slug>/plan.prev.md` (no `plan.md`, a rebuilt board → no snapshot). The current implementer is dropped, never resumed; its uncommitted work stays in the worktree.
2. Fresh `spec-planner` (*Gate 2* input, plus `mode=replan from=s<n>`, the reason verbatim, the worktree path). `question`, `arch-gap` and `split` as at gate 2.
3. `ready` and the review's status lines → `next-step.sh <slug> replan …`: review rounds as at gate 2 (scope `plan from s<n>`), then the **stop** `prompt-render.sh replan <slug> from=s<n>` (the plan summary, plus the architecture summary and the brief when their revs moved — an arch-gap during the re-plan — and a verdict with a `Needs user` item) or, asked at gate 4's stop with nothing moved, the notice `prompt-render.sh replan-notice <slug> from=s<n>` (it logs `replan=pre-authorized`).
4. On approval: for every card-to-replace's `spec-delta … added` line, re-log it on the parent card with `card-log.sh <slug> spec-delta <ID> added origin=<its origin> -- retired`, so their IDs stay retired, delete those cards, create the new ones as *Gate 2* step 4, after a stop, log `replan=approved`, render the draft PR body, spawn the implementer (*Every stage*); its brief says the worktree holds the dropped implementer's uncommitted work. The run rejoins at gate 3's branch review, gate 4 (new plan summary shown).

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
  - **split** → *Split*; the implementer narrows the entry.

Gate 3's reviewer checks every branch spec change was classified; gate 4's sync makes `spec-diff.md` the final spec, never one the orchestrator composes.

#### Split

A change needed only for something outside the goal; the user's choice at a question or a `Needs user` item. Before `s0` the raising agent (architect, planner) has already narrowed the entry when it answers `status=split reason=…`; after `s0` the reviewer only raised it, and the implementer narrows it last (below). Orchestrator resumes `spec-author` with the reason verbatim (after `s0`: the verdict item and the `review.md` path); it writes `split-issue.md`. Orchestrator files it (`gh issue create`, as in *Tracking issue*) with `Split from #<issue>` appended as its last line — at once if the tracking issue is filed, else right after gate 2 approval step 1 — appends it to the parent card's `split:`, posts one notice line `Split · <slug> · <IDs> · #<m> filed`, then resumes the architect or planner that raised it — after `s0`, the implementer with `split <ID>: narrow it`, gauntlet, the usual review. The new issue is its own later run.

**Fold in** — the user's explicit override of a split: the raising agent rewrites the `Goal:` line to the widened scope (after `s0`: it is a meaningful change, kept).


### Gate 3 — review

Before proposing a PR, whole-branch pass — cross-stage bugs, spec drift across stages, scope creep:
- Fresh `spec-reviewer`, scope `branch`. Give it base ref, artifact dir, worktree path, all stage ids. Already run as the last code stage's review (*Per-stage review*) and no code changed since → skip to gate 4.
- Orchestrator runs `gauntlet.sh` on the branch, forwards both status lines.
- The status line → `next-step.sh <slug> branch …`: findings → the same implementer fixes, then a fresh branch review, never a follow-up pass here; findings needing a user decision are relayed as questions, never fixed unasked. It logs `review=<n>-open`, then `review=clean` or `review=<n>-resolved` (the PR status line reads it).
- Minors (*Verify before an approval stop*) are resolved before `spec-author` writes `pr.md`; gauntlet re-run, `gauntlet=pass sha=<sha>` logged on the parent card.
- **A `Needs user` item left:** full lane → `prompt-render.sh gate3 <slug>`; fast lane → it rides gate 4's stop, answered by its letter, and an answer that changes code → fix, gauntlet, a fresh branch review, gate 4 again.
- **Clean:** no stop.
- **Board:** reviewer appends to `artifacts/<slug>/review.md`, keyed to stage id, and rewrites `review.verdict.md`. Parent card → `inreview/` when review starts.

### Gate 4 — pull request

- Gauntlet + gate 3 clean, *PR feedback* at `threads=0`.
- **ID collisions** — another run may have landed the same fixed IDs meanwhile: `git -C <worktree> fetch -q origin main && sh .claude/scripts/id-collisions.sh <worktree>`. `ids=collide <IDs>` → resume the implementer to renumber them on the branch (spec, test citations, `spec-delta` log), gauntlet, push, check again.
- `spec-author`, given the worktree path and the board root, syncs `spec-diff.md` from the branch's `docs/specs/`, writes `artifacts/<slug>/pr.md` in **full form** and one `follow-up-<k>.md` per thing the run noticed outside its goal (implementer cards' `follow-up` lines, `review.md`'s `follow-up — ` lines; answers `follow-ups=<n>`) (content rules in its file; `.github/PULL_REQUEST_TEMPLATE.md` carries the same sections for human PRs). Orchestrator assembles both bodies:

```sh
sh .claude/scripts/pr-body.sh <slug> full > artifacts/<slug>/pr-full.md   # exit 3 (body=refused stage-ref=…): resume spec-author to reword pr.md
```

- The full body is the draft minus `## Stages` and `## Review here`, with its own status line, the `pr.md` prose in place of `## Why` and `## Scenarios covered` in place of `## Scenarios`; its spec changes come from the synced `spec-diff.md` Revisions lines, so the tick boxes and board state drop out and the remaining sections keep their order.

- **One stop**: the author's `ready` → `next-step.sh <slug> pr …`, then `prompt-render.sh gate4 <slug>` (`pr.md`, every `follow-up-<k>.md`, the branch verdict; the plan summary after a pre-authorized re-plan). `re-plan: <why>` → *Re-plan*. Option 1 narrows the follow-ups (none given: all are filed); `stop` holds for a manual run — never pre-empt it. Approved → push, replace the draft's title and body, mark ready, log `pr=ready`:

```sh
gh pr edit <pr> --title "$(head -1 artifacts/<slug>/pr-full.md | sed 's/^# //')" --body-file <(tail -n +2 artifacts/<slug>/pr-full.md)
gh pr ready <pr>
```

- No draft exists (run predates this rule, or resumed from a board that lacks `pr`): `gh pr create` with the same title and body file instead of `edit`+`ready`.
- **Follow-ups**, right after `pr=ready`: each one the answer kept is filed as in *Tracking issue* (title = line 1, body = the rest) with `Found in #<issue>` appended as its last line; log `follow-ups=[<n>, …]` on the parent card. A dropped one is never filed.
- **CI** — wait for the PR's checks, one status line in context:

```sh
gh pr checks <pr> --watch > artifacts/<slug>/ci.log 2>&1; ci=$?
sh .claude/scripts/next-step.sh <slug> ci ci=$ci
```

  - `ci=0` → *Clean up*, which records the metrics.
  - otherwise (`next-step.sh` prints the `failed-workflow.sh` call; never raw `gh run view`) → implicated stage card → `inprogress/`, resume or spawn the implementer to fix, gauntlet, push, wait again. No stop unless a fix needs a user decision.
  - A failure the user rules unrelated to the run (e.g. also red on `main`), on the user's word only → `card-log.sh <slug> ci-waived -- <why>`, then *Clean up*.

### Clean up

The run's last step, right after CI is green — no approval stop: the issue and the PR hold everything that outlives the run, `metrics.log` its cost. The human reviews and merges on GitHub, outside the workflow.

```sh
sh .claude/scripts/run-cleanup.sh <slug>
```

It refuses (exit 1, one line) while the worktree has uncommitted changes or commits its upstream lacks, or while the parent card's last `ci=` is not 0 and no later `ci-waived` line exists (an epic is exempt); else it records the run's line in `metrics.log` (`metrics.sh`; a re-run replaces it), removes the worktree, deletes the local branch (the pushed one stays for the PR) and moves every card and `artifacts/<slug>/` into `.claude/tasks/archive/<slug>/` (never pruned; a second run of the same slug gets a dated name), printing `cleanup=done archive=<path>`. One notice line: `Done · <slug> · PR #<pr> ready · CI green · cleaned up · archived`.

### Resume an interrupted run

**Step 0.** `git -C <main checkout> pull --ff-only` before reading anything.

**No card, PR exists** (the user names the issue and its draft PR): `sh .claude/scripts/run-rebuild.sh <issue> <pr>`. `rebuild=done … next=s<n>` → the reconcile table below, then *Re-plan* from `next=` with a fresh planner (`plan.md` is never rebuilt; `next=none` → *Gate 3*). `rebuild=refused reason=no-stages` → the PR is ready: CI check, then *Clean up*. No PR → gate 1 with "take issue #n". Any other refusal → report it.

Cards outside `open/`+`done/`, no agent running = session died mid-run. Resume triggered by the user or `/spec-feature`, never automatic.

`kind: epic` with `issue` set → the epic stop was approved: continue filing at the first slice not yet in `slices:` (step 5.2 adopts one filed before the crash), never re-file one. Otherwise: no worktree on the card → died during gate 1, architecture or gate 2, nothing on disk to reconcile — resume the conversation from `spec-diff.md`/`architecture.md`/`plan.md`'s last state (`arch` set → gate 2). `issue` set but no worktree → gate 2 was approved: continue at its step 2, never re-file. Past gate 2 → table below. **Any resumed implementation spawns a fresh agent**, its brief naming the card's `lane`.

**Reconcile before acting.** Card = intent, git = fact: `sh .claude/scripts/reconcile.sh <slug>` checks worktree, branch, every `commit=`, every `done` stage and the last `gauntlet=pass` against git (`differ row=…` lines, then `reconcile=clean|differ … next=s<n>`); `gauntlet.sh` at the pass sha answers `cached` when the card told the truth. The rows git cannot answer:

| Card claims | Check | Disagreement means |
|---|---|---|
| `issue=<n>` | `issue-view.sh <n>` — open; a comment per `spec-keep=` on the parent card | missing comment: post `issue-comment.md` (*Tracking issue*) |
| `pr=<n>` | `pr-feedback.sh fetch <n> …` — its status line's `state=`/`draft=` | `pr=ready` logged, or closed/merged: the run is over — CI check, then *Clean up*; missing: draft never opened, the next push opens it |
| PR body | the PR body vs `pr-body.sh <slug> draft` | body behind the board or the branch: a move or a stage commit never rendered, `gh pr edit` with the fresh render |

Report differences first. Agree → resume. Disagree → stop and report.

A `replan from=` line with no later `replan=approved` or `replan=pre-authorized` on the parent card → the re-plan is pending: rerun *Re-plan* from step 2 with a fresh planner; `plan.prev.md` is already the snapshot.

Clean reconcile → resume only no-approval work (respawn the implementer for approved stages, CI wait and *Clean up* after `pr=ready`); halt at the first gate needing the user. Recorded `gate1`/`arch`/`gate2` approvals stay valid — the fast lane's pre-authorization too: `arch` set, `lane: fast`, no `gate2` → plan, then gate 2's *On approval* without a stop; `arch: skipped`, no `gate2` → plan, then gate 2's stop. A resumed run spawns every agent fresh (the architect, the author and the planner re-read their files).
