---
name: context-audit
description: Analyze Claude Code session transcripts for repeated full-file reads, large re-derived tool output, and other context-cost waste; recommend small scripts (like .claude/scripts/extract-section.sh) that would cut it. Use when the user asks to "audit context cost", "reduce context bloat", "what scripts should we add", "analyze tool usage", or invokes /context-audit.
---

# Context cost audit

**Concise, compact, facts only.**

Read-only analysis. Never write a script itself — recommends, user decides scope (a new script is a maintenance commitment, same "ask before" spirit as AGENTS.md's scope boundaries).

## 1. Find the transcripts

Session logs: `~/.claude/projects/<project-slug>/*.jsonl`, one file per session, JSON Lines. `<project-slug>` = cwd path with `/` → `-`. Ask the user for scope — this session only, last N sessions, or all — default last 5 (recent behavior matters more than a year-old habit).

## 2. Tally tool usage

Per session, extract every `Read` tool_use `input.file_path`, every `Bash` `input.command`, every `Grep` `input.pattern`/`input.path`. `jq`, no other dependency assumed:

```sh
jq -r 'select(.message.content != null) | .message.content[]?
  | select(.type=="tool_use" and .name=="Read") | .input.file_path' \
  ~/.claude/projects/<slug>/*.jsonl | sort | uniq -c | sort -rn
```

Same shape for `Bash` (`.input.command`) and `Grep` (`.input.pattern`).

## 2b. Rank cost by tool

Which tool's *results* eat the most context, not just which is called most — a `Bash` call piping raw `gh`/`git` JSON back verbatim costs far more per call than a `Grep`. Join each `tool_result` to its `tool_use` by id, sum result size per tool name, sort descending:

```sh
jq -s '
  ( [.[] | .message.content[]? | select(.type=="tool_use") | {key: .id, value: .name}] | from_entries ) as $names
  | [.[] | .message.content[]? | select(.type=="tool_result") | {name: ($names[.tool_use_id] // "unknown"), size: ((.content | tostring) | length)}]
  | group_by(.name) | map({name: .[0].name, calls: length, chars: (map(.size) | add)})
  | sort_by(-.chars)[]
  | "\(.name)\t\(.calls) calls\t~\(.chars/4|floor) tokens"
' ~/.claude/projects/<slug>/*.jsonl
```

`chars/4` is a rough tokens estimate, not exact — good enough for ranking. A `Bash` entry near the top is the signal to drill into *which* commands (section 2's tally) are driving it — that's the candidate list for section 3's script check.

## 3. Flag waste patterns

- **Full-file re-read, same file, ≥3 times in one session, no `Edit` in between two of them** — file didn't change, content was re-fetched anyway. Check the file's own structure (headings, JSON keys, log sections) against what the agent quoted right after each Read — if only one part was ever used, that's an `extract-section.sh` candidate (or a JSON/log equivalent: `jq`, `awk` slice). `.claude/scripts/token-rank.sh <file>...` (if the project has it) gives a quick rough cost per file to prioritize which repeat offenders are worth fixing first.
- **Large file (`wc -l` it) read whole when the same heading/keyword recurs across sessions** — same pattern, seen over time instead of in one session.
- **Repeated identical `Bash` command** — deterministic, cacheable output (`git log`, a version check) re-run instead of reasoned from a prior result already in context. Not a script problem — note separately, the fix is behavioral, don't recommend a script for it.
- **`Grep` with a large match count, re-run later in the same session with an added `-l`/path filter** — the first call's output was too big to use directly; narrowing came a call too late.
- **`Bash` tool dominates section 2b's ranking** — check which commands (section 2's tally) drive it. A raw `gh`/`git`/`curl`/API command whose full JSON or log gets dumped into context, when only a few fields ever get used, is a script candidate the same shape as `.claude/scripts/failed-workflow.sh`/`issue-view.sh` (compact, pre-filtered output instead of raw dump). Repeated across sessions, not just once, before recommending.

## 4. Report

Ranked table: file/pattern, count, rough cost (`lines × occurrences` for Reads), proposed fix. One line each — no praise, no summary paragraph. Precede it with section 2b's per-tool ranking, unabridged — it's the map of where cost concentrates before the fix-level detail. `.claude/scripts/extract-section.sh` (if the project has it) already covers markdown-heading slicing — recommend *extending its use*, never a duplicate script, when the pattern already fits it. Only propose a new script when the data shape doesn't (JSON, log tail, CSV column, etc).

Do not create anything here. If the user approves a specific recommendation, draft that one script the same way `extract-section.sh` was built: POSIX `sh`, single file, one clear job, tested against a real sample before reporting done.
