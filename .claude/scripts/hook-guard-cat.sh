#!/bin/sh
# PreToolUse guard on the Bash tool: blocks an unpiped `cat` of a markdown
# file, or of any file over LARGE_LINES lines, and redirects to the
# extract-section.sh convention (markdown) or the Read tool / sed -n range
# (anything else) — AGENTS.md's Conventions section already says this;
# this hook makes the bypass fail instead of silently dumping a whole file
# into context.
#
# Reads a PreToolUse hook payload on stdin, writes a deny-decision JSON
# object on stdout when it blocks, nothing when it doesn't.
set -eu

LARGE_LINES=80

input=$(cat)
name=$(printf '%s' "$input" | jq -r '.tool_name // empty')
[ "$name" = "Bash" ] || exit 0

cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
[ -n "$cmd" ] || exit 0

case "$cmd" in
  *'|'*) exit 0 ;;   # already piped through a filter downstream
  *'<<'*) exit 0 ;;  # heredoc (e.g. commit message body), not a file cat
esac

segments=$(printf '%s' "$cmd" | tr ';&' '\n\n')

offender=""
reason=""

old_ifs=$IFS
IFS='
'
for seg in $segments; do
  case "$seg" in
    *cat\ *)
      rest=$(printf '%s' "$seg" | sed -n 's/^[[:space:]]*cat[[:space:]]\{1,\}//p')
      [ -n "$rest" ] || continue
      for tok in $rest; do
        case "$tok" in
          -*) continue ;;
        esac
        [ -f "$tok" ] || continue
        case "$tok" in
          *.md)
            offender="$tok"
            reason="Whole-file cat of a .md file bypasses this repo's extract-section.sh convention (AGENTS.md Conventions). Run: sh .claude/scripts/list-sections.sh $tok to see headings, then sh .claude/scripts/extract-section.sh '<heading>' $tok for just what's needed. Genuinely need the whole document (rewrite/restructure)? Use the Read tool instead of Bash cat."
            ;;
          *)
            lines=$(wc -l < "$tok" 2>/dev/null || echo 0)
            if [ "$lines" -gt "$LARGE_LINES" ]; then
              offender="$tok"
              reason="Whole-file cat of a $lines-line file bypasses this repo's Conventions (AGENTS.md: filter shell output, use Read/sed -n for a range instead of a full Bash cat). Use the Read tool (with offset/limit if only part is needed) or 'sed -n START,ENDp' $tok."
            fi
            ;;
        esac
        [ -z "$offender" ] || break 2
      done
      ;;
  esac
done
IFS=$old_ifs

if [ -n "$offender" ]; then
  jq -n --arg reason "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $reason}}'
fi
exit 0
