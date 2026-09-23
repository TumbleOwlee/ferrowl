---
name: spec-review
description: Independent second-developer review of an open PR against its ticket's spec — standalone entrypoint, no shared session with whoever implemented it. Input is the ticket (holds the approved spec plus any updates landed during implementation). Output is the full gate-3-style review for the developer's own approval gate before manual QA and merge. Use when a different developer needs to review a finished branch/PR before it merges.
---

# Spec review (independent, PR-facing)

**Concise, compact, facts only.**

Criteria and output shape are `spec-reviewer.md`'s (`## Four axes, reported separately`, `## Output`); the reviewer is never the implementer (`.claude/AGENTS.workflow.md` `### Verify before an approval stop`). This skill supplies gate 3's *inputs* for a reviewer outside the implementing session; it restates no criteria. Conflict → `.claude/AGENTS.workflow.md` wins.

## Gather inputs — no shared session

- **Ticket** — `sh .claude/scripts/extract-section.sh '### Gate 1b — tracking issue. Stop for approval.' .claude/AGENTS.workflow.md` names the tracker and how to read it. Ticket is self-contained: full current normative text, including updates landed via "Reconcile the spec". This *is* the approved spec — `artifacts/<slug>/spec-diff.md` may not exist on this machine or may be gone.
- **Branch/PR** — from the ticket's linked PR, or ask the user for PR number/branch. Read the PR with `bash .claude/scripts/pr-view.sh <number>`.
- **Base ref** — PR's target branch (usually `main`).

## Run the review

Create a scratch `artifacts/<slug>/` and write the ticket's normative text into its `spec-diff.md` (one `## <ID>` per requirement, same shape as gate 1) so the reviewer reads it the way it reads every spec diff; `review.md` and `review.verdict.md` land there too. Spawn `spec-reviewer` (`.claude/agents/spec-reviewer.md`) with: scope `branch` (not a wave), the base ref, the artifact dir, the checkout path, every stage in scope. No `plan.md` exists on this side — say so; spec fidelity is checked against `spec-diff.md` alone. It reads its own rules (its file names the `AGENTS.md` headings); give it nothing more, never the issue/PR number. It answers with a status line only (`### Agent hand-off`).

The requirement is a reader that never held the implementer's context: a fresh session may review inline with the same axes and rigor; a session that implemented any of it spawns.

## Output

Point the developer at `review.verdict.md` (`sh .claude/scripts/show-file.sh`), the full `review.md` on request. Findings needing a user decision are flagged, not resolved.

## Stop condition

Report and stop. Approving is the developer's own gate before manual QA and merge — no PR edits, no merge, no board.
