---
name: context-audit
description: Analyze Claude Code session transcripts for repeated full-file reads, large re-derived tool output, and other context-cost waste; recommend small scripts (like .claude/scripts/extract-section.sh) and, where a waste pattern is a repeated bypass of an existing convention, propose a list of concrete fixes (a hook, a convention edit) rather than a script alone. Use when the user asks to "audit context cost", "reduce context bloat", "what scripts should we add", "analyze tool usage", or invokes /context-audit.
---

# Context cost audit

**Concise, compact, facts only.**

Read-only. Never write a script — recommend, user decides (a script is a maintenance commitment; same "ask before" spirit as AGENTS.md's scope boundaries).

## 1. Find the transcripts

`~/.claude/projects/<project-slug>/*.jsonl`, one per session, JSON Lines. `<project-slug>` = cwd with `/` → `-`. Ask scope — this session, last N, all — default last 5.

## 2. Tally tool usage

Per session: every `Read` `input.file_path`, `Bash` `input.command`, `Grep` `input.pattern`/`input.path`. `jq` only:

```sh
jq -r 'select(.message.content != null) | .message.content[]?
  | select(.type=="tool_use" and .name=="Read") | .input.file_path' \
  ~/.claude/projects/<slug>/*.jsonl | sort | uniq -c | sort -rn
```

Same for `Bash` (`.input.command`) and `Grep` (`.input.pattern`).

## 2b. Rank cost by tool

Which tool's *results* eat most context, not which is called most. Join each `tool_result` to its `tool_use` by id, sum result size per tool, sort descending:

```sh
jq -s '
  ( [.[] | .message.content[]? | select(.type=="tool_use") | {key: .id, value: .name}] | from_entries ) as $names
  | [.[] | .message.content[]? | select(.type=="tool_result") | {name: ($names[.tool_use_id] // "unknown"), size: ((.content | tostring) | length)}]
  | group_by(.name) | map({name: .[0].name, calls: length, chars: (map(.size) | add)})
  | sort_by(-.chars)[]
  | "\(.name)\t\(.calls) calls\t~\(.chars/4|floor) tokens"
' ~/.claude/projects/<slug>/*.jsonl
```

`chars/4` is rough, good enough for ranking. `Bash` near the top → drill into which commands (section 2) drive it; that's section 3's script-candidate list.

## 3. Flag waste patterns

- **Full-file re-read, same file, ≥3 times in one session, no `Edit` between two of them.** Check the file's structure (headings, JSON keys, log sections) against what was quoted after each Read — only one part ever used = `extract-section.sh` candidate (or `jq`/`awk` slice for JSON/log). `.claude/scripts/token-rank.sh <file>...` (if present) ranks repeat offenders by cost.
- **Large file (`wc -l`) read whole when the same heading/keyword recurs across sessions** — same pattern over time.
- **Repeated identical `Bash` command** — deterministic output (`git log`, version check) re-run instead of reasoned from context. Behavioral fix, not a script; note separately.
- **`Grep` with a large match count, re-run later with `-l`/path filter** — narrowing came a call too late.
- **`Bash` dominates section 2b** — a raw `gh`/`git`/`curl`/API command dumping full JSON/log when only a few fields get used is a script candidate shaped like `.claude/scripts/failed-workflow.sh`/`issue-view.sh` (compact, pre-filtered). Require repetition across sessions before recommending.

## 4. Report

Ranked table: file/pattern, count, rough cost (`lines × occurrences` for Reads), proposed fix. One line each, no praise, no summary. Precede with section 2b's per-tool ranking, unabridged. `.claude/scripts/extract-section.sh` (if present) already covers markdown-heading slicing — recommend *extending its use*, never a duplicate. New script only when the data shape doesn't fit (JSON, log tail, CSV column).

## 5. Propose enforcement when the pattern is a bypassed convention

A recurring waste pattern that `AGENTS.md` Conventions already forbid (e.g. raw Bash `cat` of a whole `.md`/large file when `extract-section.sh`/`sed -n`/Read exist) is not a missing script — the fix exists and is routed around. For each, propose as a numbered list, most costly first:

- **A `PreToolUse` hook** detecting the exact bypass shape at the tool-call boundary and denying with a message pointing at the intended path — shape of `.claude/scripts/hook-guard-shell.sh` if present (`ls .claude/scripts/hook-guard-*.sh`; extend an existing guard covering an overlapping shape before adding one). State matcher (tool name), detection condition, redirect message.
- **A convention-wording gap**, if the bypass happened because the bullet didn't cover the observed shape (wrong tool, file type, ambiguous wording) — quote current bullet and proposed edit.

One line each: pattern → proposed fix → which file changes (`.claude/settings.json` + new/edited `.claude/scripts/hook-guard-*.sh`, or the Conventions bullet). Create or edit nothing; list, let the user approve.
