---
name: spec-request
description: PO-facing entrypoint — grill a requirement into an approved brief (scenarios, best-effort requirements and edge cases under local IDs) via direct conversation, then open a tracking issue. Stops there; never touches the task board, a worktree, the architecture step or gate 2 onward. Use when a product owner brings a requirement and just needs it turned into an approved brief and a ticket for a developer to pick up.
---

# Spec request (PO-facing)

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for gate 1 (the brief) and the tracking issue — the headings in the table below, followed exactly. Conflict → `.claude/AGENTS.workflow.md` wins.

Single-session, no resume: skip gate 1's **Board** bullet (except storing a named issue as `request.md`), its lane question (always the full-lane brief approval), and every "record … on the parent card" step. `spec-author` still needs a scratch `artifacts/<slug>/` (deleted once the ticket exists). The ticket is the only surviving artifact (*Tracking issue*): its spec carries local IDs and a `### Deferred` list, the architect and planner refine both, and the planner fixes the IDs. Approved spec text never lands in `docs/specs/` here. The parent-card fields `issue-body.sh` reads are absent here; its status line reads `**Spec** rev N` alone.

## Where each step lives

Headings of `.claude/AGENTS.workflow.md`, one pull each (`AGENTS.md` `## Conventions — reading`).

| Step | Heading |
|---|---|
| Orchestrator role — read once | `### Principles` |
| Agent hand-off | `### Agent hand-off` |
| Gate 1 — brief, dialogue only, no board | `### Gate 1 — brief` |
| Tracking issue — runs right after gate 1 approval here (no architecture, no plan); `spec-author` writes `issue.md`, the orchestrator assembles the body with `issue-body.sh`, shows it, files it on approval | `### Tracking issue` |

## Stop condition

An author answer with `size=epic` is shown as information (`Size:` line) and the brief is approved and filed as one ticket all the same: the epic-or-single choice needs the architecture step, so it is the developer's, at `/spec-feature`'s gate 1.

Stop once the ticket exists — never the architecture step, gate 2, or a worktree. A developer picks it up via `/spec-feature` naming the issue: gate 1 reuses the ticket's brief (the author asks only what the developer adds) and the run continues at the architecture step.
