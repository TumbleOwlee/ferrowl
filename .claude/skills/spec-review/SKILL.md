---
name: spec-review
description: Independent second-developer review of an open PR against its ticket's spec — standalone entrypoint, no shared session with whoever implemented it. Input is the ticket (holds the approved spec plus any updates landed during implementation). Output is the full gate-3-style review for the developer's own approval gate before manual QA and merge. Use when a different developer needs to review a finished branch/PR before it merges.
---

# Spec review (independent, PR-facing)

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md`'s `### Gate 3` defines what a review checks (spec fidelity, standards, TDD honesty) and how (`spec-reviewer` agent, never the implementer). This skill supplies gate 3's *inputs* for a reviewer outside the implementing session; it restates no criteria. Conflict → `.claude/AGENTS.workflow.md` wins.

## Gather inputs — no shared session, no artifacts dir

- **Ticket** — `sh .claude/scripts/extract-section.sh '### Gate 1b — tracking issue. Stop for approval.' .claude/AGENTS.workflow.md` names the tracker and how to read it. Ticket is self-contained: full current normative text, including updates landed via "Reconcile the spec". This *is* the approved spec — `artifacts/<slug>/spec-diff.md` may not exist on this machine or may be gone.
- **Branch/PR** — from the ticket's linked PR, or ask the user for PR number/branch.
- **Base ref** — PR's target branch (usually `main`).

## Run the review

Spawn `spec-reviewer` (`.claude/agents/spec-reviewer.md`) with: spec text from the ticket, `git diff <base>...<head>` scoped to the whole branch (gate 3, not a wave), every stage in scope. It reads its own rules (`.claude/AGENTS.core.md`); give it nothing more, never the issue/PR number.

Give it a scratch `artifacts/<slug>/` for `review.md` and `review.verdict.md`; it answers with a status line only (`### Agent hand-off`). Reviewing yourself instead of spawning is fine — same axes, same rigor. Requirement is an independent read, not necessarily a subagent.

## Output

`review.md`: axis-grouped, severity-tagged, no praise. `review.verdict.md`: open findings only, capped. Point the developer at the verdict (`show-file.sh`), the full file on request; findings needing a user decision are flagged, not resolved.

## Stop condition

Report and stop. Approving is the developer's own gate before manual QA and merge — no PR edits, no merge, no board.
