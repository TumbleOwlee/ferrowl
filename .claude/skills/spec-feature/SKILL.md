---
name: spec-feature
description: Drive one behavior change through the repo's gated spec-driven TDD workflow — spec diff, tracking issue, implementation plan, worktree implementation, independent review, PR. Use when starting a feature, fix, or any change to observable behavior in a repo whose AGENTS.md defines these gates.
---

# Spec-driven feature run

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for every gate, the task board, and the subagents — follow it exactly, pulled one heading at a time per the table below (`AGENTS.md`'s own `## Workflow` heading is now a short pointer to that file). This skill is only the invocation entrypoint; it does not restate that procedure. Conflict between this file and `.claude/AGENTS.workflow.md` → `.claude/AGENTS.workflow.md` wins.

## Before anything else

Check `.claude/tasks/`. Cards outside `open/`+`done/` = a run was interrupted → `.claude/AGENTS.workflow.md`'s *Resume an interrupted run*, don't start fresh.

## Where each step lives

Pull one section at a time, never the whole file: `sh .claude/scripts/extract-section.sh '<heading>' .claude/AGENTS.workflow.md`.

| Step | Heading |
|---|---|
| Parent card | `### Gate 1 — spec diff. Orchestrator runs this itself. Stop for approval.` (the **Board:** bullet) |
| Gate 1 — spec diff | `### Gate 1 — spec diff. Orchestrator runs this itself. Stop for approval.` |
| Gate 1b — tracking issue | `### Gate 1b — tracking issue. Orchestrator runs this itself. Stop for approval.` |
| Gate 2 — implementation plan | `### Gate 2 — implementation plan. Stop for approval.` |
| Implement, stage by stage | `### Implement, stage by stage` |
| Reconcile the spec | `### Reconcile the spec` |
| Gate 3 — independent review | `### Gate 3 — review. Stop for approval.` |
| Gate 4 — pull request | `### Gate 4 — pull request. Stop for approval.` |
| Merge and clean up | `### Merge` |
| Resume a dead run | `### Resume an interrupted run` |
