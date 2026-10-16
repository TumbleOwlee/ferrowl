---
name: spec-review
description: Independent second-developer review of an open PR against its ticket's spec, with no shared session with the implementer. Input is the ticket (approved spec plus later revisions); output is the full gate-3-style review for the developer's own approval gate before manual QA and merge. Use when a different developer must review a finished branch/PR before merge.
---

# Spec review (independent, PR-facing)

**Concise, compact, facts only.**

Criteria and output shape are `spec-reviewer.md`'s (`## Axes, reported separately`, `## Spec changes`, `## Output`); this skill supplies gate 3's *inputs* for a reviewer outside the implementing session. Conflict → `.claude/AGENTS.workflow.md` wins.

## Gather inputs — no shared session

- **Ticket** — the GitHub issue (`issue-view.sh`, `AGENTS.md` `## Conventions — GitHub`). Ticket is self-contained: the issue's `## Requirements` (local IDs) and `## Scenarios` are the spec as filed, its comments the changes kept since; the PR body (`pr-view.sh`) maps local to fixed IDs (`## Requirements`, Issue column) and holds `## Architecture` and `## Decisions`. Changes classified as detail are only on the branch, in `docs/specs/`. `artifacts/<slug>/spec-diff.md` may not exist on this machine or may be gone.
- **Branch/PR** — from the ticket's linked PR, or ask the user for PR number/branch.
- **Base ref** — PR's target branch (usually `main`).

## Run the review

Create a scratch `artifacts/<slug>/` and write the ticket's normative text into its `spec-diff.md` (one `## S<n> — <title>` section per scenario, one `## <ID> — <kind>` section per row of the PR's `## Requirements` table, the approved changes from the comments applied, same shape as gate 1) and the PR's `## Architecture` into `architecture.md` (the table → `## Components`, `Public surface:` → `## Public surface`; the PR has no interfaces) with its `## Decisions` so the reviewer reads them the way it reads every run's; `review.md` and `review.verdict.md` land there too. Spawn `spec-reviewer` (`.claude/agents/spec-reviewer.md`) with: scope `branch`, the base ref, the artifact dir, the checkout path. No `plan.md` and no cards exist on this side — say so: no stage ids, every finding's stage column is `—`, no cards (`spec-reviewer.md` `## Spec changes`); the ticket's spec with its approved comments applied (`spec-diff.md`) is the approved baseline. Architecture conformance covers `## Components` (Role cells name key members and variants), `## Public surface` and the PR's decisions; signatures are checked against the diff. Give it nothing more, never the issue/PR number. It answers with a status line only (`### Agent hand-off`).

A fresh session may review inline with the same axes and rigor; a session that implemented any of it spawns.

## Output

Point the developer at `review.verdict.md` (`sh .claude/scripts/show-file.sh`), the full `review.md` on request.

## Stop condition

Report and stop. Approving is the developer's own gate before manual QA and merge — no PR edits, no merge, no board.
