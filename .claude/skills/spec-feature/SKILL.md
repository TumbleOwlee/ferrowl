---
name: spec-feature
description: Drive one behavior change through the repo's gated spec-driven TDD workflow — brief, architecture, implementation plan and tracking issue, worktree implementation, independent review, PR. Use when starting a feature, fix, or any change to observable behavior in a repo whose AGENTS.md defines these gates.
---

# Spec-driven feature run

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for every gate, the task board, and the subagents — follow exactly, one heading at a time per the table. Conflict → `.claude/AGENTS.workflow.md` wins.

## Before anything else

Check `.claude/tasks/` against `### Resume an interrupted run` before starting fresh.

## Where each step lives

Headings of `.claude/AGENTS.workflow.md`, one pull each (`AGENTS.md` `## Conventions — reading`).

| Step | Heading |
|---|---|
| Orchestrator role — read once per run | `### Principles` |
| Branch/worktree/agent/plan rules — read once per run | `### Principles — implementation` |
| What runs before any approval stop | `### Verify before an approval stop` |
| Card files, states, artifacts directory | `### Task board` |
| Agent hand-off (status lines, file paths) | `### Agent hand-off` |
| Approval prompt — the fixed shape of every stop, built from `show-file.sh`'s lines | `### Approval prompt` |
| Relayed question — an agent's batch of decisions | `### Relayed question` |
| Lane — fast or full, proposed by size, settled at the first approval stop | `### Lane — fast or full` |
| Gate 1 — brief: grilling, scenarios, best-effort entries | `### Gate 1 — brief` |
| Architecture — structure the plan must reach | `### Architecture` |
| Gate 2 — plan and tracking issue | `### Gate 2 — plan and tracking issue` |
| Tracking issue — draft, file, changelog comments, rewrite at gate 4 | `### Tracking issue` |
| Implement — every lane: spawn brief, draft PR | `#### Every stage` |
| Implement — fast lane | `#### Fast lane` |
| Implement — full lane, sequential mode | `#### Sequential`, `#### Per-stage review`, `#### Stage stop` |
| Implement — full lane, parallel mode | `#### Parallel waves`, `#### Stage stop` |
| PR feedback — draft PR comments back into the run, before every approval stop once the draft exists | `### PR feedback` |
| Spec changes — who edits the spec when, detail vs meaningful, split | `### Spec changes` |
| Gate 3 — independent review | `### Gate 3 — review` |
| Gate 4 — pull request | `### Gate 4 — pull request` |
| After the human merges on GitHub — worktree and artifacts | `### After merge` |
| Resume a dead run | `### Resume an interrupted run` |
