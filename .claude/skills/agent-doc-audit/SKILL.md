---
name: agent-doc-audit
description: Check every agent-facing markdown file this project's agents read (AGENTS.md, .claude/AGENTS.core.md, .claude/agents/*.md, .claude/skills/*/SKILL.md, docs/specs/**) for prose bloat and for whether their headings let extract-section.sh pull one section instead of the whole file; propose a splitting plan where an agent is forced to read content only relevant to a different agent. Use when the user asks to "audit agent docs", "check doc splitting", "propose a split plan", or invokes /agent-doc-audit.
---

# Agent-facing doc split audit

**Concise, compact, facts only.**

Read-only analysis. Never split or edit a file itself — proposes, user decides (splitting is a maintenance commitment and a churn risk to every cross-reference, same "ask before" spirit as AGENTS.md's scope boundaries). Companion to `context-audit`: that one finds re-reads from session history; this one finds structural waste from the files themselves, no session history needed.

## 1. Scope: agent-facing files

`AGENTS.md`, `.claude/AGENTS.core.md`, `.claude/agents/*.md`, `.claude/skills/*/SKILL.md`, `docs/specs/**/*.md`, any file a skill/agent instruction names with "Read". Skip human-facing docs agents rarely open whole (`PRD.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `README.md`) unless an agent's own instructions cite one of them.

## 2. Build the read-map

For each agent/skill file, grep its own text for "Read `X`" / "pull `Y` section" and note which *sections* (not just files) it actually names — `.claude/agents/spec-planner.md`, `spec-implementer.md`, `spec-reviewer.md`, and each `.claude/skills/*/SKILL.md` are the read-map's source, since they're where "who reads what" is declared. Build a table: file → heading → which agent(s) cite it.

## 3. Flag split candidates

- **Heading cited by exactly one agent type, sharing a file with headings cited by others** — that agent pays for the whole file (or the whole file minus what `extract-section.sh` slices out) to get its one part. Split candidate: move the section to its own file, or confirm it's already sliceable and just needs a pointer update.
- **No heading structure at all, or headings that don't match what agents actually ask for** — e.g. a `##` per topic but an agent's instructions say "the part about X" where X spans two headings. `extract-section.sh` pulls exactly one heading's span; content an agent needs split across headings defeats it. Fix: re-head the file along the boundary agents actually cite, don't just add more headings blindly. `list-sections.sh <file>` (if the project has it) dumps the actual heading list to check against — faster than reading the file to see what's there.
- **A whole-file read where the citing agent instructions already say "pull only section Y"** — the doc structure is fine, the instruction just doesn't point `extract-section.sh` at it yet. Not a split, a wiring fix — note separately from real split candidates.
- **Section reused verbatim by every agent type that touches the file** (e.g. `AGENTS.core.md`'s existing carve-out) — correctly merged already, not a candidate; note it as a working example, don't propose undoing it.

## 4. Check prose density per candidate section

Before proposing a split, the section earning it has to justify the extra file. Scan for:

- Sentences restating what a heading, list, or code block already shows.
- Hedging / filler words (basically, essentially, in order to, it's worth noting).
- Any explanation of *what* code does where a reader could get that from names alone — only *why* (non-obvious constraint, workaround, invariant) earns a sentence.
- Repeated instructions across two files where one could `@`-include or cross-reference the other instead of restating.

`.claude/scripts/token-rank.sh <file>...` (if the project has it) gives rough per-file cost to prioritize which candidates matter most. Trim-first, split-second: a bloated section split in two is two bloated files — flag the trim regardless of whether the split also happens.

## 5. Report

Two tables, most-costly first (`token-rank.sh` order where available):

**Split candidates** — file, heading(s), citing agent(s), current shared readers, proposed new file/heading, rough token cost avoided per off-target read.

**Prose flags** — file, heading, one-line problem (restatement / hedge / filler / duplicate-of-<file>), no fix text needed — the problem statement is the fix.

One line each, no praise, no summary paragraph. If a proposed split's new file would end up under ~10 lines, say so and recommend a heading instead of a file — splitting past that point adds cross-reference upkeep for a slice `extract-section.sh` could already pull cheaply from the shared file.

Do not create or edit anything here. If the user approves a specific split, make the new file's headings match exactly what the citing agent instructions already name (or update those instructions in the same change, never leave them pointing at a heading that moved).
