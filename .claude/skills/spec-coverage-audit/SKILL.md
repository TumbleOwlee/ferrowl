---
name: spec-coverage-audit
description: Find code with no requirement covering it, and get an approved spec diff (new requirements, and new capability areas if needed) that catches docs/specs up to what the code already does. If no governed spec directory exists yet, scaffolds one first (capability areas, requirement-ID convention, routing pointer in AGENTS.md/CLAUDE.md) — nothing else of the full workflow. Does not check for stale/untested requirements (spec with no citing test) — only reports that gap and asks whether to handle it. Use when the user asks to "find missing specs", "audit spec coverage", "what's undocumented", "backfill specs for this code", or wants "just the spec directory" added to an existing project.
---

# Spec coverage audit

**Concise, compact, facts only.**

Direction: **code → no spec**. Reverse (**spec → no test**) is out of scope — step 7 asks, doesn't act.

Self-contained: every file written comes from this skill's own `templates/` and `scripts/` (next to `SKILL.md`). Copy the folder alone into any project and it works. Either directory missing → incomplete copy; stop, ask for the whole `spec-coverage-audit/` directory.

Guard rails: ask, never guess (`AskUserQuestion` for every unknown); never write a requirement without explicit approval; never fabricate — every entry traces to code actually read; never overwrite a file without asking keep/overwrite/merge.

## 0. Check for a governed spec directory

An `AGENTS.md` (or `CLAUDE.md`) routing table pointing at a spec directory, and that directory with a `README.md`. Both present → step 1. Either missing → **step 0a** scaffolds, then step 1 on the fresh structure.

### 0a. Scaffold — only when step 0 found nothing

Never asks about stack, CI, hooks, trackers; never sets up build/test/lint; never adds gates 2-4. Only makes a spec directory exist and be findable.

1. **Agent-instructions file**: `AGENTS.md`, else `CLAUDE.md`, is the append target for item 6. Neither → item 6 creates a minimal `AGENTS.md`.
2. **Spec directory location**: `AskUserQuestion`, default `docs/specs`. Below: `<spec-dir>`.
3. **Capability areas**: one `AskUserQuestion` round, 2-6 areas, each with directory name (lowercase, short), one-line "covers", requirement ID prefix — two letters + `-R-` (`FR-R-nnn`), unique, never `NF-R-*` (reserved). Ground in the visible source tree (top-level modules/packages). Also ask per-area starting files — default `requirements.md` + `edge-cases.md`; `api-contract.md` for a public surface, `data-contract.md` for a wire/file format. User doesn't know the areas → don't invent: only `non-functional-requirements.md` + `README.md`, a TBD routing row; step 4 proposes areas from code.
4. **Confirm before writing**: compact summary — location, areas with prefixes and files, files to create, file to append to. One yes; then write without further prompting.
5. **Write** from `templates/` (substitute placeholders):

   | Bundled template | Output |
   |---|---|
   | `templates/README.md.tmpl` | `<spec-dir>/README.md` |
   | `templates/non-functional-requirements.md` | `<spec-dir>/non-functional-requirements.md` (copy as-is) |
   | `templates/area/requirements.md.tmpl` | `<spec-dir>/<area>/requirements.md` — per area |
   | `templates/area/edge-cases.md.tmpl` | `<spec-dir>/<area>/edge-cases.md` — per area |
   | `templates/area/api-contract.md.tmpl` | `<spec-dir>/<area>/api-contract.md` — areas that asked |
   | `templates/area/data-contract.md.tmpl` | `<spec-dir>/<area>/data-contract.md` — areas that asked |

   `{{PROJECT_NAME}}`: repo directory name. `{{AREA_TABLE}}`: one row per area linking `./<area>/`, plus the fixed non-functional row. `{{AREA_TITLE}}`/`{{AREA_COVERS}}`/`{{AREA_PREFIX}}` from item 3. Copy this skill's `scripts/extract-id.sh`, `extract-section.sh`, `list-sections.sh`, `token-rank.sh` into `.claude/scripts/`, `chmod +x`, skip existing. Grep every written file for `{{` — a leak is a missed placeholder.
6. **Append the routing section** to the file from item 1 (or write the minimal `AGENTS.md`): exactly three things — (a) routing table, one row per area plus the `non-functional-requirements.md` row:

   ```
   | Task touches | Read | ID prefix |
   |---|---|---|
   | <area covers> | [`<area>`](<spec-dir>/<area>/) | `<PREFIX>-R-*` |
   | Cross-cutting (platforms, performance, security, versioning, testing) | [`non-functional-requirements.md`](<spec-dir>/non-functional-requirements.md) | `NF-R-*` |
   ```

   (b) ID convention — stable, append-only, never renumbered or reused once retired, cited in commits/PRs/tests, one requirement per physical line; (c) minimal spec-handling note — before adding/changing a requirement in `<spec-dir>/`, propose the diff and get explicit approval from the project owner, then write. Nothing about plans, worktrees, review. A new minimal `AGENTS.md` = those three items plus a one-line title.

## 1. Scope

Optional area or path argument narrows the run. None → every area in the routing table.

## 2. Map each area to source

Per area: guess its source directory (area name vs `src/<area>`, or the routing table's "covers" wording), confirm via one `AskUserQuestion` before reading — auditing the wrong directory silently is worse than one question.

## 3. Read and compare — per area

Semantic read, not a citation grep: a citation grep catches only tests missing an ID, never code with *no test at all* — the gap that matters most.

1. Read the area's `requirements.md` (and `edge-cases.md`, so a documented intentional gap isn't re-flagged).
2. Read the source in that area.
3. Compare observable behavior vs stated `shall` requirements. Flag uncovered behavior.

Anything fitting no area in scope (or none at all): note separately, never fold into the nearest area.

## 4. Draft the diff

Draft only; write nothing yet.

- **Gaps in an existing area**: new `<PREFIX>-R-nnn` entries, one rule each (README rule 9), next free number (append-only — check the highest existing, never reuse/renumber). Testable, observable wording: `**<PREFIX>-R-001** — The <subject> shall <observable outcome> when <condition>.`
- **Code fitting no area**: propose a new area inline — directory name, one-line "covers", unique prefix (two letters + `-R-`, not colliding with existing or `NF-R-*`) — then its first requirements.

## 5. Get approval

Present the full draft — grouped by area, new areas called out — for one explicit approval. No tracking issue, no commit — code exists, nothing to track.

Rejected/edited items: adjust, re-confirm before writing. Never write a requirement the user didn't see in this form.

## 6. Write

New area approved: write from `templates/area/*.tmpl` as step 0a item 5 (substitute placeholders, only the files needed), append a routing row:

```
| <area covers> | [`<area>`](<spec-dir>/<area>/) | `<PREFIX>-R-*` |
```

Then append approved entries to each `requirements.md` — one per physical line, never wrapped.

Report what was written (files, IDs, new area/scaffold). Stop — no commit, issue, PR. User reviews and commits.

## 7. Ask about the other direction

Last: ask whether a follow-up should report requirements with no citing test (spec → no test), and whether to add to this skill or build separately. Record a clear answer; act on nothing.
