# spec-planner — re-plan

Read by `spec-planner` only when spawned with `mode=replan from=s<n>` (its `## Re-plan`); everything in `.claude/agents/spec-planner.md` still applies.

## Re-plan

Spawned mid-run with `mode=replan from=s<n>`, the reason verbatim and the worktree path besides the usual input: the stages before `s<n>` are committed on the branch and fixed. Read `plan.md` whole (no `plan.md`, a rebuilt board: read `plan.summary.md` and the stage cards instead, and write `plan.md` whole — each committed stage as its heading plus one line `Committed: <sha>`; the `## Shared` tree lists every stage), then in the worktree `git log main..HEAD --oneline` (what landed) and `git status --short` plus `git diff` (the work the stopped implementer left; the new stages start from it — reuse it or have a step discard it, by name). Rewrite every stage from `s<n>` on — edit, merge, split, add, reorder, drop — and their `## Shared` tree entries; every section before `s<n>` stays byte for byte. In `## Shared`, every `Decisions:` line stays, those an implementer appended included. New stages take ids from `s<n>` upward in plan order; a dropped stage's ID→test rows move to the stage that now pins them.

Once `s0` landed, the spec is the branch's `docs/specs/`, not `spec-diff.md`: edit neither and assign no ID. A stage that needs a spec change names it in its steps; its implementer lands it as a spec change the reviewer classifies. `from=s0` (nothing landed yet) → the usual gate 2 rules. A structural change → `status=arch-gap` (`spec-planner.md` `## Input`). Questions as in `spec-planner.md` `## Interview before drafting`. Rewrite `plan.summary.md` whole (bump `rev`; the rows of committed stages unchanged, `Decide:` naming what the re-plan changed and why), answer `status=ready`.
