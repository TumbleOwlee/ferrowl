---
name: spec-request
description: PO-facing entrypoint — derive spec text from a requirement via direct conversation, then open a tracking issue. Stops there; never touches the task board, a worktree, or gate 2 onward. Use when a product owner brings a requirement and just needs it turned into an approved spec diff and a ticket for a developer to pick up.
---

# Spec request (PO-facing)

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for gate 1 and the tracking issue — the headings in the table below, followed exactly. Conflict → `.claude/AGENTS.workflow.md` wins.

Single-session, no resume: skip gate 1's **Board** bullet, its lane question (always the full-lane approval), and every "record … on the parent card" step — no `open/<slug>.md`, no task card. `spec-author` still needs a scratch `artifacts/<slug>/` (deleted once the ticket exists). The ticket is the only surviving artifact; approved spec text lives in its self-contained body (`## Spec (rev N)`, one list item per entry), not `docs/specs/`. The parent-card fields `issue-body.sh` reads are absent here; its status line reads `Spec rev N` alone.

## Where each step lives

Headings of `.claude/AGENTS.workflow.md`, one pull each (`AGENTS.md` `## Conventions — reading`).

| Step | Heading |
|---|---|
| Orchestrator role — read once | `### Principles` |
| Agent hand-off | `### Agent hand-off` |
| Gate 1 — spec diff, dialogue only, no board | `### Gate 1 — spec diff. Stop for approval.` |
| Tracking issue — runs right after gate 1 approval here (no plan); `spec-author` writes `issue.md`, the orchestrator assembles the body with `issue-body.sh` (no plan decisions, no stage count), shows it, files it on approval | `### Tracking issue` |

## Stop condition

Stop once the ticket exists — never gate 2, `spec-planner`, or a worktree. Developer picks it up via `/spec-feature` or `spec-planner`.
