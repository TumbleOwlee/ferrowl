---
name: spec-request
description: PO-facing entrypoint — derive spec text from a requirement via direct conversation, then open a tracking issue. Stops there; never touches the task board, a worktree, or gate 2 onward. Use when a product owner brings a requirement and just needs it turned into an approved spec diff and a ticket for a developer to pick up.
---

# Spec request (PO-facing)

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for gate 1 and 1b — read `### Gate 1` and `### Gate 1b`, follow exactly. This skill is the entrypoint only; it restates nothing. Conflict → `.claude/AGENTS.workflow.md` wins.

Single-session, no resume: skip gate 1's **Board** bullet — no `open/<slug>.md`, no task card. `spec-author` still needs a scratch `artifacts/<slug>/` (deleted once the ticket exists). The ticket is the only surviving artifact; approved spec text lives in its self-contained body, not `docs/specs/` (main holds spec only for existing code). Orchestrator relays and files (`### Agent hand-off`); drafts nothing.

## Where each step lives

One section at a time: `sh .claude/scripts/extract-section.sh '<heading>' .claude/AGENTS.workflow.md`.

| Step | Heading |
|---|---|
| Orchestrator role — read once | `### Principles` |
| Agent hand-off | `### Agent hand-off` |
| Gate 1 — spec diff, dialogue only, no board | `### Gate 1 — spec diff. Stop for approval.` |
| Gate 1b — tracking issue | `### Gate 1b — tracking issue. Stop for approval.` |

## Stop condition

Stop once the ticket exists — never gate 2, `spec-planner`, or a worktree. Developer picks it up via `/spec-feature` or `spec-planner`.
