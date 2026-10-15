# spec-author — split issue and gate 4

Read by `spec-author` only when resumed for a split or for gate 4; everything in `.claude/agents/spec-author.md` still applies.

## Split issue — `split-issue.md`

```
# <title, plain language>

## Goal
<what the user gets, one or two lines>
<what happens today, one line>

## Found
- reproduction
  - <step>
  - <observed>
- cause
  - <fact>
- constraint
  - <what a fix needs, one fact per line>
- parent assumes
  - <what the parent's narrowed entry now states>
  - <what stays unfixed until this lands>

## Existing spec
- <ID>, <ID> — <what they say>
  - <gap, one line>
  - likely to change | cited
```

Existing IDs only, never a planned one. No issue number anywhere — the orchestrator prepends the status line.

## Gate 4 — sync, then `pr.md`

**Sync first.** In the worktree, `git diff main...HEAD -- docs/specs` is the run's final spec. Rewrite `spec-diff.md` to match it: one `## <ID>` section per entry the branch added, changed or dropped, fixed IDs only, scenarios kept, `## Deferred` removed; bump the rev and add `- rev N (<date>) — synced from the branch: <IDs that differ from the filed text> (spec-author)` (`issue-body.sh` skips it), then one dated line per `### Spec change` comment on the issue (`issue-view.sh`), `- rev N (<date>) — <ID> kept: <choice> (user)` — these become the issue's `## Decisions`. The orchestrator rewrites the issue body from it.

**Then `pr.md`**, full form. Inputs: `spec-diff.md`, `plan.md`, `review.md`, `gauntlet.log` (its coverage line), `git log main..HEAD --stat` in the worktree. Line 1 = `issue.md`'s line 1. The orchestrator prepends the status line and appends the architecture and the closing line; write none of them. Answer `status=ready file=artifacts/<slug>/pr.md`.

```
# <title>

## Why
<what the user gets, one line>
<what it cost today, one line>

## Changes
- <crate> <module> — <what changed> (<IDs it serves>)
- docs/specs/<area> — <n> entries added, <n> changed
- README — <what>                                            ← when touched

## Tests
<n> of <m> entries pinned
- <ID> — <exception: pinned by a real-view test | not unit-testable per docs/specs/README.md | …>

## Verification beyond the gauntlet
- <manual run, what was done and seen>
- not run: <what and who waived it> | none

## Decisions
- <plan-level decision> (<user | chosen>)
```

`## Changes` per crate module, never per file. `## Tests` lists exceptions only. `not run:` is always present. `## Decisions` = `plan.md`'s decisions only; spec decisions are in the issue.
