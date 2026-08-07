---
name: spec-review
description: Independent second-developer review of an open PR against its ticket's spec — standalone entrypoint, no shared session with whoever implemented it. Input is the ticket (holds the approved spec plus any updates landed during implementation). Output is the full gate-3-style review for the developer's own approval gate before manual QA and merge. Use when a different developer needs to review a finished branch/PR before it merges.
---

# Spec review (independent, PR-facing)

**Concise, compact, facts only.**

`AGENTS.md`'s `### Gate 3` defines what a review checks (spec fidelity, standards, TDD honesty) and how (`spec-reviewer` agent, never the implementer). This skill only supplies gate 3's *inputs* for a reviewer who wasn't in the implementing session — it does not restate the criteria. Conflict between this file and `AGENTS.md` → `AGENTS.md` wins.

## Gather inputs — no shared session, no artifacts dir

- **Ticket** — `sh .claude/scripts/extract-section.sh '### Gate 1b — tracking issue. Orchestrator runs this itself. Stop for approval.' AGENTS.md` names this project's tracker and how to read it. The ticket is self-contained: full current normative text, including any updates the orchestrator landed via "Reconcile the spec" mid-implementation. This *is* the approved spec — don't look for `artifacts/<slug>/spec-diff.md`; it may not exist on this machine, or may already be gone (worktree/board cleanup on the original developer's side).
- **Branch/PR** — from the ticket's linked PR, or ask the user for the PR number/branch if the ticket doesn't carry one.
- **Base ref** — the PR's target branch (usually `main`).

## Run the review

Spawn `spec-reviewer` (`.claude/agents/spec-reviewer.md`) with: spec text from the ticket, `git diff <base>...<head>` scoped to the whole branch (gate 3, not a wave), every stage in scope. It reads its own rules (`.claude/AGENTS.core.md`) itself — give it nothing more, never the issue/PR number.

Reviewing it yourself instead of spawning is fine — same three axes, same rigor. The requirement is an independent read, not necessarily a subagent.

## Output

Same shape as gate 3: axis-grouped, severity-tagged, no praise. Findings needing a user decision are flagged, not resolved.

## Stop condition

Report and stop. Approving this review is the developer's own gate before manual QA and merge — outside this skill's scope: no PR edits, no merge, no board.
