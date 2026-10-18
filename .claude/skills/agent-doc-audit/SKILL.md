---
name: agent-doc-audit
description: Audit every agent-facing markdown file (AGENTS.md, .ferrule/guide/*.md, .claude/agents/*.md, .claude/skills/*/SKILL.md, docs/specs/**) for prose bloat, instructions stated more than once, and headings ferrule spec section cannot slice; propose splits where an agent must read another agent's content. Use for "audit agent docs", "check doc splitting", "propose a split plan", or /agent-doc-audit.
---

# Agent-facing doc split audit

**Concise, compact, facts only.**

Read-only. Never split or edit — propose, user decides (a split churns every cross-reference). Companion to `context-audit`: that finds re-reads from session history; this finds structural waste from the files themselves.

## 1. Scope: agent-facing files

`AGENTS.md`, `.ferrule/guide/*.md` (ferrowl's overrides of the `ferrule guide` topics; the topics themselves are ferrule's), project agents and skills under `.claude/` (the generated `fe-*` files are ferrule's: findings on them go to ferrule), `docs/specs/**/*.md`, any file a skill/agent instruction names with "Read". Skip human-facing docs (`PRD.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `README.md`) unless an agent's instructions cite one.

## 2. Build the read-map

For each agent/skill file, grep its text for "Read `X`" / "pull `Y` section"; note which *sections* it names. `ferrule guide planner`, `ferrule guide implementer`, `ferrule guide reviewer` (with the project's overrides), and each `.claude/skills/*/SKILL.md` are the source — they declare who reads what. Table: file → heading → citing agent(s).

## 3. Flag split candidates

- **Heading cited by exactly one agent type, sharing a file with headings cited by others** — that agent pays for the whole file (or whole minus `ferrule spec section`'s slice). Candidate: move to its own file, or confirm already sliceable and fix the pointer.
- **No heading structure, or headings not matching what agents ask for** — e.g. an agent says "the part about X" where X spans two headings. `ferrule spec section` pulls one heading's span. Fix: re-head along the boundary agents cite, not more headings blindly. `ferrule spec sections <file>` dumps the heading list.
- **Whole-file read where the citing instruction already says "pull only section Y"** — structure fine, instruction not yet wired to `ferrule spec section`. Wiring fix, not a split; note separately.
- **Section reused by every agent type touching the file** (e.g. `AGENTS.md`'s `## Conventions — code`, pulled by heading) — correctly shared; note as a working example, don't propose copying it out.

## 4. Check prose density per candidate section

Before proposing a split, the section must justify the extra file. Scan for:

- Sentences restating a heading, list, or code block.
- Hedging/filler (basically, essentially, in order to, it's worth noting).
- Explaining *what* code does where names suffice — only *why* (non-obvious constraint, workaround, invariant) earns a sentence.
- Every trim: zero information loss. Fewer words, same fact — never a dropped constraint, edge case, or rule.

`wc -c <file>... | sort -rn` gives per-file cost (~chars/4 tokens) to prioritize. Trim first, split second: a bloated section split is two bloated files — flag the trim regardless.

## 5. Check duplication across all in-scope files

Every file from step 1, not only split candidates. An instruction stated twice costs every reader twice and drifts: one copy gets edited, the other keeps the stale rule.

- **Same rule, several places** — same do/don't in two sections or two files, whatever the wording (e.g. "one requirement, one physical line" in `AGENTS.md` and again in `docs/specs/README.md`). Flag every copy but one; the survivor is the one readers of the others already reach (`@`-include, cross-reference, or the section the read-map shows they pull anyway).
- **Rule restated inside its own file** — a heading's rule repeated in a later section, a summary bullet repeating the body, a "reminder" of an earlier constraint.
- **Agent file repeating the router** — `.claude/agents/*.md` or a `SKILL.md` restating an `AGENTS.md` convention its reader already loads. Pointer to the heading, not a copy.
- **Not duplication** — a rule that must be read in isolation by a reader who never loads the other file, or a requirement-ID cross-reference. Note the reason only if a naive grep would flag it.

Method: for each rule-bearing sentence, grep its distinctive noun or verb across the step 1 set; a second hit with the same meaning is a flag.

## 6. Report

Three tables, most costly first (`wc -c` order):

**Split candidates** — file, heading(s), citing agent(s), current shared readers, proposed new file/heading, rough token cost avoided per off-target read.

**Prose flags** — file, heading, one-line problem (restatement / hedge / filler), no fix text — the problem statement is the fix.

**Duplicates** — the rule in a few words, every file:heading holding a copy, which copy to keep and why (that reader already loads it).

One line each, no praise, no summary; an empty table is "none". A proposed split under ~10 lines: say so, recommend a heading instead — `ferrule spec section` already pulls it cheaply.

If the user approves a split, make the new file's headings match exactly what the citing instructions name (or update those instructions in the same change).
