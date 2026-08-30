---
name: spec-feature
description: Drive one behavior change through the repo's gated spec-driven TDD workflow — spec diff, tracking issue, implementation plan, worktree implementation, independent review, PR. Use when starting a feature, fix, or any change to observable behavior in a repo whose AGENTS.md defines these gates.
---

# Spec-driven feature run

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for every gate, the task board, and the subagents — follow exactly, one heading at a time per the table (`AGENTS.md`'s `## Workflow` is a pointer to it). This skill is the entrypoint only; it restates nothing. Conflict → `.claude/AGENTS.workflow.md` wins.

## Before anything else

Check `.claude/tasks/`. Cards outside `open/`+`done/` = interrupted run → `### Resume an interrupted run`, don't start fresh.

## Where each step lives

One section at a time: `sh .claude/scripts/extract-section.sh '<heading>' .claude/AGENTS.workflow.md`.

| Step | Heading |
|---|---|
| Orchestrator role, branch/worktree/agent rules — read once per run | `### Principles` |
| What runs before any approval stop | `### Verify before an approval stop` |
| Agent hand-off (status lines, file paths) | `### Agent hand-off` |
| Parent card | `### Gate 1 — spec diff. Stop for approval.` (the **Board:** bullet) |
| Gate 1 — spec diff | `### Gate 1 — spec diff. Stop for approval.` |
| Gate 1b — tracking issue | `### Gate 1b — tracking issue. Stop for approval.` |
| Gate 2 — implementation plan | `### Gate 2 — implementation plan. Stop for approval.` |
| Implement, stage by stage | `### Implement, stage by stage` |
| Reconcile the spec | `### Reconcile the spec` |
| Gate 3 — independent review | `### Gate 3 — review. Stop for approval.` |
| Gate 4 — pull request | `### Gate 4 — pull request. Stop for approval.` |
| Merge and clean up | `### Merge` |
| Resume a dead run | `### Resume an interrupted run` |
