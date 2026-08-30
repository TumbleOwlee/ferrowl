---
name: spec-coverage-audit
description: Find code with no requirement covering it, and get an approved spec diff (new requirements, and new capability areas if needed) that catches docs/specs up to what the code already does. If no governed spec directory exists yet, scaffolds one first (capability areas, requirement-ID convention, routing pointer in AGENTS.md/CLAUDE.md) — nothing else of the full workflow. Does not check for stale/untested requirements (spec with no citing test) — only reports that gap and asks whether to handle it. Use when the user asks to "find missing specs", "audit spec coverage", "what's undocumented", "backfill specs for this code", or wants "just the spec directory" added to an existing project.
---

# Spec coverage audit

**Concise, compact, facts only.**

Direction covered: **code → no spec**. The reverse (**spec → no test**, stale/orphaned requirement) is out of scope for this run — step 7 asks the user about it, doesn't act on it.

Self-contained: every file this skill writes comes from its own `templates/` and `scripts/` directories (next to this `SKILL.md`). Copy this skill's folder alone into any project and it works. Missing either directory → the folder was copied incompletely; stop, ask for the whole `spec-coverage-audit/` directory.

Guard rails: ask, never guess (`AskUserQuestion` for every unknown); never write a requirement without explicit approval first; never fabricate — every proposed entry must trace to code you actually read, not a guess; never overwrite a file without asking keep/overwrite/merge first.

## 0. Check for a governed spec directory

An `AGENTS.md` (or `CLAUDE.md`) routing table pointing at a spec directory, and that directory with a `README.md`. Both present → step 1. Either missing → **step 0a** scaffolds them, then step 1 continues on the fresh (empty) structure — the audit is what fills it.

### 0a. Scaffold — only when step 0 found nothing

Never asks about stack, CI, hooks, or trackers; never sets up build/test/lint; never adds gates 2-4. Only makes a spec directory exist and be findable.

1. **Agent-instructions file**: `AGENTS.md`, else `CLAUDE.md`, is the append target for item 6. Neither → item 6 creates a minimal `AGENTS.md`.
2. **Spec directory location**: `AskUserQuestion`, default `docs/specs`. Every path below is `<spec-dir>`.
3. **Capability areas**: one `AskUserQuestion` round, 2-6 areas, each with a directory name (lowercase, short), a one-line "covers" description, and a requirement ID prefix — two letters + `-R-` (`FR-R-nnn`), unique, never `NF-R-*` (reserved for non-functional). Ground the proposal in the source tree you can see (top-level modules/packages), not an abstract prompt. Also ask per-area starting files — default `requirements.md` + `edge-cases.md`; `api-contract.md` for a public surface, `data-contract.md` for a wire or file format. User doesn't know the areas → don't invent them: only `non-functional-requirements.md` + `README.md`, a TBD routing row; step 4 proposes areas from the code.
4. **Confirm before writing**: compact summary — location, areas with prefixes and files, files to create, file to append to. One yes; then write without further prompting.
5. **Write** from this skill's `templates/` (substitute placeholders):

   | Bundled template | Output |
   |---|---|
   | `templates/README.md.tmpl` | `<spec-dir>/README.md` |
   | `templates/non-functional-requirements.md` | `<spec-dir>/non-functional-requirements.md` (static, copy as-is) |
   | `templates/area/requirements.md.tmpl` | `<spec-dir>/<area>/requirements.md` — once per area |
   | `templates/area/edge-cases.md.tmpl` | `<spec-dir>/<area>/edge-cases.md` — once per area |
   | `templates/area/api-contract.md.tmpl` | `<spec-dir>/<area>/api-contract.md` — only areas that asked |
   | `templates/area/data-contract.md.tmpl` | `<spec-dir>/<area>/data-contract.md` — only areas that asked |

   `{{PROJECT_NAME}}`: repo directory name, no question. `{{AREA_TABLE}}`: one row per area linking `./<area>/`, plus the fixed non-functional row. `{{AREA_TITLE}}`/`{{AREA_COVERS}}`/`{{AREA_PREFIX}}` from item 3. Copy this skill's `scripts/extract-id.sh`, `extract-section.sh`, `list-sections.sh`, `token-rank.sh` into `.claude/scripts/`, `chmod +x`, skipping any that already exist. Grep every written file for `{{` — a leak is a missed placeholder.
6. **Append the routing section** to the file from item 1 (or write the minimal `AGENTS.md`): exactly three things — (a) the routing table, one row per area plus the `non-functional-requirements.md` row:

   ```
   | Task touches | Read | ID prefix |
   |---|---|---|
   | <area covers> | [`<area>`](<spec-dir>/<area>/) | `<PREFIX>-R-*` |
   | Cross-cutting (platforms, performance, security, versioning, testing) | [`non-functional-requirements.md`](<spec-dir>/non-functional-requirements.md) | `NF-R-*` |
   ```

   (b) the ID convention — IDs stable and append-only, never renumbered or reused once retired, cited in commits/PRs/tests, one requirement per physical line; (c) a minimal spec-handling note — before adding or changing a requirement in `<spec-dir>/`, propose the diff and get explicit approval from whoever owns the project, then write it. Nothing about plans, worktrees, or review. A new minimal `AGENTS.md` is those three items plus a one-line title — nothing else.

## 1. Scope

Accept an optional area or path argument to narrow the run. No argument → every area in the routing table.

## 2. Map each area to source

Per area in scope: guess its source directory (area name vs `src/<area>`-style match, or the routing table's own wording for what that area "covers"), then confirm or correct the guess via one `AskUserQuestion` before reading anything — auditing the wrong directory silently is worse than one extra question.

## 3. Read and compare — per area

This is a semantic read, not a grep for citation markers: a citation-grep proxy only catches tests missing an ID, and misses code with *no test at all*, which is exactly the gap that matters most here.

For the confirmed source directory:

1. Read the area's `requirements.md` (and `edge-cases.md`, so a documented intentional gap isn't re-flagged as missing).
2. Read the source code in that area.
3. Reason about what observable behavior exists vs. what's stated as a `shall` requirement. Flag behavior with nothing covering it.

For anything that doesn't fit any area in scope (or any area at all): note it separately — handled below in this same step, not folded silently into the nearest area.

## 4. Draft the diff

Do not write anything yet — only draft.

- **Gaps within an existing area**: new `<PREFIX>-R-nnn` "shall" entries, next free number for that area's prefix (append-only — check the highest existing number in that `requirements.md`, never reuse or renumber). Testable, observable-outcome wording, e.g. `**<PREFIX>-R-001** — The <subject> shall <observable outcome> when <condition>.`
- **Code fitting no existing area**: propose a new area inline — directory name (lowercase, short), one-line "covers" description, and a unique requirement ID prefix (two letters + `-R-`, not colliding with any existing prefix or `NF-R-*`) — then draft its first requirements the same way.

## 5. Get approval

Present the full draft — grouped by area, new areas called out separately — for one explicit approval: derive the spec text, stop before writing. No tracking issue, no commit — code already exists, so there's no implementation step to track.

Rejected/edited items: adjust and re-confirm before writing. Never write a requirement the user didn't see in this form.

## 6. Write

Any new area approved in step 4: write it from this skill's `templates/area/*.tmpl` exactly as step 0a item 5 does (substitute placeholders, only the files that area needs), and append a routing-table row:

```
| <area covers> | [`<area>`](<spec-dir>/<area>/) | `<PREFIX>-R-*` |
```

Then append the approved requirement entries to each `requirements.md` — one requirement per physical line, never wrapped.

Report what was written (files touched, requirement IDs added, any new area or scaffold created). Then stop — no commit, no issue, no PR. The user reviews and commits manually.

## 7. Ask about the other direction

Last thing before ending the run — ask the user: should a follow-up also report requirements with no citing test (spec → no test, stale/unverified), and if so, should that be added to this skill or built as a separate one? Record the answer if they give a clear direction; don't act on it in this run either way.
