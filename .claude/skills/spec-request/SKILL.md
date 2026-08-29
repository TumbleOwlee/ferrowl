---
name: spec-request
description: PO-facing entrypoint — derive spec text from a requirement via direct conversation, then open a tracking issue. Stops there; never touches the task board, a worktree, or gate 2 onward. Use when a product owner brings a requirement and just needs it turned into an approved spec diff and a ticket for a developer to pick up.
---

# Spec request (PO-facing)

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for gate 1 and gate 1b — read `### Gate 1` and `### Gate 1b` and follow them exactly. This skill is only the invocation entrypoint; it does not restate that procedure. Conflict between this file and `.claude/AGENTS.workflow.md` → `.claude/AGENTS.workflow.md` wins.

Single-session, no resume: skip gate 1's **Board** bullet — no `open/<slug>.md` or task card. `spec-author` still needs a place to write: give it a scratch `artifacts/<slug>/` (deleted once the ticket exists). The ticket from gate 1b is the only artifact that survives; approved spec text lives in its self-contained body, not `docs/specs/` (main only holds spec for code that already exists). The orchestrator relays and files (`### Agent hand-off`); it drafts nothing itself.

## Where each step lives

Pull one section at a time, never the whole file: `sh .claude/scripts/extract-section.sh '<heading>' .claude/AGENTS.workflow.md`.

| Step | Heading |
|---|---|
| Agent hand-off | `### Agent hand-off` |
| Gate 1 — spec diff, dialogue only, no board | `### Gate 1 — spec diff. Stop for approval.` |
| Gate 1b — tracking issue | `### Gate 1b — tracking issue. Stop for approval.` |

## Stop condition

Stop once the ticket exists — never gate 2, never `spec-planner`, never a worktree. Developer picks it up later via `/spec-feature` or `spec-planner` directly; out of scope here.
