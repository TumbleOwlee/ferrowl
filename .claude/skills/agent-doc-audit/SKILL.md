---
name: agent-doc-audit
description: Check every agent-facing markdown file this project's agents read (AGENTS.md, .claude/AGENTS.workflow.md, .claude/agents/*.md, .claude/skills/*/SKILL.md, docs/specs/**) for prose bloat, for the same instruction stated more than once (within a file or across files), and for whether their headings let extract-section.sh pull one section instead of the whole file; propose a splitting plan where an agent is forced to read content only relevant to a different agent. Use when the user asks to "audit agent docs", "check doc splitting", "propose a split plan", or invokes /agent-doc-audit.
---

# Agent-facing doc split audit

**Concise, compact, facts only.**

Read-only. Never split or edit — propose, user decides (a split is a maintenance commitment and churns every cross-reference; same "ask before" spirit as AGENTS.md's scope boundaries). Companion to `context-audit`: that finds re-reads from session history; this finds structural waste from the files themselves.

## 1. Scope: agent-facing files

`AGENTS.md`, `.claude/AGENTS.workflow.md`, `.claude/agents/*.md`, `.claude/skills/*/SKILL.md`, `docs/specs/**/*.md`, any file a skill/agent instruction names with "Read". Skip human-facing docs (`PRD.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `README.md`) unless an agent's instructions cite one.

## 2. Build the read-map

For each agent/skill file, grep its text for "Read `X`" / "pull `Y` section"; note which *sections* it names. `.claude/agents/spec-planner.md`, `spec-implementer.md`, `spec-reviewer.md`, and each `.claude/skills/*/SKILL.md` are the source — they declare who reads what. Table: file → heading → citing agent(s).

## 3. Flag split candidates

- **Heading cited by exactly one agent type, sharing a file with headings cited by others** — that agent pays for the whole file (or whole minus `extract-section.sh`'s slice). Candidate: move to its own file, or confirm already sliceable and fix the pointer.
- **No heading structure, or headings not matching what agents ask for** — e.g. an agent says "the part about X" where X spans two headings. `extract-section.sh` pulls one heading's span. Fix: re-head along the boundary agents cite, not more headings blindly. `list-sections.sh <file>` (if present) dumps the heading list.
- **Whole-file read where the citing instruction already says "pull only section Y"** — structure fine, instruction not yet wired to `extract-section.sh`. Wiring fix, not a split; note separately.
- **Section reused by every agent type touching the file** (e.g. `AGENTS.md`'s `## Conventions — code`, pulled by heading) — correctly shared; note as a working example, don't propose copying it out.

## 4. Check prose density per candidate section

Before proposing a split, the section must justify the extra file. Scan for:

- Sentences restating a heading, list, or code block.
- Hedging/filler (basically, essentially, in order to, it's worth noting).
- Explaining *what* code does where names suffice — only *why* (non-obvious constraint, workaround, invariant) earns a sentence.
- Every trim: zero information loss. Fewer words, same fact — never a dropped constraint, edge case, or rule.

`.claude/scripts/token-rank.sh <file>...` (if present) gives per-file cost to prioritize. Trim first, split second: a bloated section split is two bloated files — flag the trim regardless.

## 5. Check duplication across all in-scope files

Every file from step 1, not only split candidates. An instruction stated twice costs every reader twice and drifts: one copy gets edited, the other keeps the stale rule.

- **Same rule, several places** — same do/don't in two sections or two files, whatever the wording (e.g. "one requirement, one physical line" in `AGENTS.md` and again in `docs/specs/README.md`). Flag every copy but one; the survivor is the one readers of the others already reach (`@`-include, cross-reference, or the section the read-map shows they pull anyway).
- **Rule restated inside its own file** — a heading's rule repeated in a later section, a summary bullet repeating the body, a "reminder" of an earlier constraint.
- **Agent file repeating the router** — `.claude/agents/*.md` or a `SKILL.md` restating an `AGENTS.md` convention its reader already loads. Pointer to the heading, not a copy.
- **Not duplication** — a rule that must be read in isolation by a reader who never loads the other file, or a requirement-ID cross-reference. Note the reason only if a naive grep would flag it.

Method: for each rule-bearing sentence, grep its distinctive noun or verb across the step 1 set; a second hit with the same meaning is a flag.

## 6. Report

Three tables, most costly first (`token-rank.sh` order where available):

**Split candidates** — file, heading(s), citing agent(s), current shared readers, proposed new file/heading, rough token cost avoided per off-target read.

**Prose flags** — file, heading, one-line problem (restatement / hedge / filler), no fix text — the problem statement is the fix.

**Duplicates** — the rule in a few words, every file:heading holding a copy, which copy to keep and why (that reader already loads it).

One line each, no praise, no summary. A proposed split under ~10 lines: say so, recommend a heading instead — `extract-section.sh` already pulls it cheaply.

Report only improvements found. Empty table → "none" — never a reason, never "already good".

Create or edit nothing here. If the user approves a split, make the new file's headings match exactly what the citing instructions name (or update those instructions in the same change).
