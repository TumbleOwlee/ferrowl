---
name: spec-request
description: PO-facing entrypoint — derive spec text from a requirement via direct conversation, then open a tracking issue. Stops there; never touches the task board, a worktree, or gate 2 onward. Use when a product owner brings a requirement and just needs it turned into an approved spec diff and a ticket for a developer to pick up.
---

# Spec request (PO-facing)

**Concise, compact, facts only.**

`AGENTS.md` is authority for gate 1 and gate 1b — read `### Gate 1` and `### Gate 1b` and follow them exactly. This skill is only the invocation entrypoint; it does not restate that procedure. Conflict between this file and `AGENTS.md` → `AGENTS.md` wins.

Single-session, no resume: skip gate 1's **Board** bullet entirely — no `open/<slug>.md`, `artifacts/<slug>/`, `spec-diff.md`, or task card. The ticket from gate 1b is the only artifact produced; approved spec text lives in its self-contained body, not `docs/specs/` (main only holds spec for code that already exists).

## Where each step lives

Pull one `AGENTS.md` section at a time, never the whole file: `sh .claude/scripts/extract-section.sh '<heading>' AGENTS.md`.

| Step | Heading |
|---|---|
| Gate 1 — spec diff, dialogue only, no board | `### Gate 1 — spec diff. Orchestrator runs this itself. Stop for approval.` |
| Gate 1b — tracking issue | `### Gate 1b — tracking issue. Orchestrator runs this itself. Stop for approval.` |

## Stop condition

Stop once the ticket exists — never gate 2, never `spec-planner`, never a worktree. Developer picks it up later via `/spec-feature` or `spec-planner` directly; out of scope here.
