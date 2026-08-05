---
name: spec-feature
description: Drive one behavior change through the repo's gated spec-driven TDD workflow — spec diff, tracking issue, implementation plan, worktree implementation, independent review, PR. Use when starting a feature, fix, or any change to observable behavior in a repo whose AGENTS.md defines these gates.
---

# Spec-driven feature run

`AGENTS.md` is authority for every gate, the task board, and the subagents —
read its `## Workflow` section and follow it exactly. This skill is only the
invocation entrypoint; it does not restate that procedure. Conflict between
this file and `AGENTS.md` → `AGENTS.md` wins.

## Before anything else

Check `.claude/tasks/`. Cards outside `open/`+`done/` = a run was interrupted
→ `AGENTS.md`'s *Resume an interrupted run*, don't start fresh.

## Where each step lives

| Step | `AGENTS.md` section |
|---|---|
| Parent card | `## Workflow` → Gate 1 board bullet |
| Gate 1 — spec diff | `### Gate 1` |
| Gate 1b — tracking issue | `### Gate 1b` |
| Gate 2 — implementation plan | `### Gate 2` |
| Implement, stage by stage | `### Implement, stage by stage` |
| Reconcile the spec | `### Reconcile the spec` |
| Gate 3 — independent review | `### Gate 3` |
| Gate 4 — pull request | `### Gate 4` |
| Merge and clean up | `### Merge` |
| Resume a dead run | `### Resume an interrupted run` |
