---
name: spec-feature
description: Drive one behavior change through the repo's gated spec-driven TDD workflow — brief, architecture, implementation plan and tracking issue, worktree implementation, independent review, PR. Use when starting a feature, fix, or any change to observable behavior in a repo whose AGENTS.md defines these gates.
---

# Spec-driven feature run

**Concise, compact, facts only.**

`.claude/AGENTS.workflow.md` is authority for every gate, the task board, and the subagents — follow exactly. Conflict → `.claude/AGENTS.workflow.md` wins.

## Before anything else

Check `.claude/tasks/` against `### Resume an interrupted run` before starting fresh.

## Where each step lives

Headings of `.claude/AGENTS.workflow.md`, one pull each (`AGENTS.md` `## Conventions — reading`).

| Step | Heading |
|---|---|
| Orchestrator role — read once per run | `### Principles` |
| Branch/worktree/agent/plan rules — read once per run | `### Principles — implementation` |
| What runs before any approval stop | `### Verify before an approval stop` |
| Task board — cards: files, frontmatter, log lines, states | `#### Cards` |
| Artifacts directory — every file of `artifacts/<slug>/` | `#### Artifacts` |
| Agent hand-off (status lines, file paths) | `### Agent hand-off` |
| Approval prompt — every stop rendered by `prompt-render.sh`, its two answer shapes | `### Approval prompt` |
| Relayed question — an agent's batch of decisions | `### Relayed question` |
| Lane — fast or full, proposed by size, settled at the first approval stop | `### Lane — fast or full` |
| Gate 1 — brief: grilling, scenarios, best-effort entries | `### Gate 1 — brief` |
| Architecture — structure the plan must reach | `### Architecture` |
| Epic — a feature too big for one run: skeleton, slices, child issues | `### Epic` |
| Slice run — a run on one child issue of an epic | `#### Slice run` |
| Gate 2 — plan and tracking issue | `### Gate 2 — plan and tracking issue` |
| Tracking issue — draft, file, changelog comments | `### Tracking issue` |
| Implement — every lane: spawn brief, draft PR | `#### Every stage` |
| Implement — fast lane | `#### Fast lane` |
| Implement — full lane | `#### Full lane`, `#### Per-stage review`, `#### Stage stop` |
| PR feedback — draft PR comments back into the run, before every approval stop once the draft exists | `### PR feedback` |
| Spec changes — who edits the spec when, detail vs meaningful | `### Spec changes` |
| Split — a change needed only outside the goal; fold in | `#### Split` |
| Gate 3 — independent review | `### Gate 3 — review` |
| Gate 4 — pull request, mark ready, CI | `### Gate 4 — pull request` |
| Clean up — after the PR is ready and CI is green; the run's end | `### Clean up` |
| Resume a dead run | `### Resume an interrupted run` |
