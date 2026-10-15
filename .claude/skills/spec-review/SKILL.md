---
name: spec-review
description: Independent second-developer review of an open PR against its ticket's spec, with no shared session with the implementer. Input is the ticket (approved spec plus later revisions); output is the full gate-3-style review for the developer's own approval gate before manual QA and merge. Use when a different developer must review a finished branch/PR before merge.
---

# Spec review (independent, PR-facing)

**Concise, compact, facts only.**

Criteria and output shape are `spec-reviewer.md`'s (`## Axes, reported separately`, `## Spec changes`, `## Output`); this skill supplies gate 3's *inputs* for a reviewer outside the implementing session. Conflict → `.claude/AGENTS.workflow.md` wins.

## Gather inputs — no shared session

- **Ticket** — the GitHub issue (`issue-view.sh`, `AGENTS.md` `## Conventions — GitHub`). Ticket is self-contained: its body's `## Spec (rev N)` is the spec as filed (its scenarios and entries), its comments the meaningful changes the user approved since, and `## Architecture` the approved structure. Changes classified as detail are only on the branch, in `docs/specs/`. `artifacts/<slug>/spec-diff.md` may not exist on this machine or may be gone.
- **Branch/PR** — from the ticket's linked PR, or ask the user for PR number/branch.
- **Base ref** — PR's target branch (usually `main`).

## Run the review

Create a scratch `artifacts/<slug>/` and write the ticket's normative text into its `spec-diff.md` (one `## S<n> — <title>` section per scenario, one `## <ID> — <kind>` section per row of the body's `## Spec`, the approved changes from the comments applied, same shape as gate 1) and its `## Architecture` into `architecture.md` (`## Components` with the table, `## Public surface`) so the reviewer reads them the way it reads every run's; `review.md` and `review.verdict.md` land there too. Spawn `spec-reviewer` (`.claude/agents/spec-reviewer.md`) with: scope `branch`, the base ref, the artifact dir, the checkout path. No `plan.md` and no cards exist on this side — say so: no stage ids, every finding's stage column is `—`, no `spec-delta` log to check; the ticket's spec with its approved comments applied (`spec-diff.md`) is the approved baseline, and every branch `docs/specs/` text differing from it is a spec change to classify. The ticket carries no architecture decisions: architecture conformance covers `## Components` and `## Public surface` only. Give it nothing more, never the issue/PR number. It answers with a status line only (`### Agent hand-off`).

A fresh session may review inline with the same axes and rigor; a session that implemented any of it spawns.

## Output

Point the developer at `review.verdict.md` (`sh .claude/scripts/show-file.sh`), the full `review.md` on request.

## Stop condition

Report and stop. Approving is the developer's own gate before manual QA and merge — no PR edits, no merge, no board.
