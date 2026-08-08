#!/bin/sh
# PreToolUse guard on the Bash tool: catches shell-output bypasses of
# AGENTS.md's Conventions ("never read a whole file when only part is
# needed" / "filter shell output before it lands in context") and denies
# them instead of letting the dump land silently. Three shapes:
#   - unpiped `cat` of a markdown file, or of any file over LARGE_LINES lines
#   - unpiped `git show`/`git diff` with no --stat and no pathspec
#   - unpiped `find` with -type f/d and no -name/-path/-iname/-regex
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

  case "$seg" in
    *git\ show\ *|*git\ diff\ *)
      case "$seg" in
        *--stat*) ;;   # already narrowed to a summary
        *' -- '*) ;;   # already scoped to a pathspec
        *:*) ;;        # git show <ref>:<path> blob form
        *)
          offender="$seg"
          reason="Unfiltered 'git show'/'git diff' bypasses this repo's Conventions (AGENTS.md: filter shell output before it lands in context). Add --stat first, or scope with a pathspec ('-- <path>'), or pipe through head/grep — rather than dumping the full diff/show."
          ;;
      esac
      ;;
  esac
  [ -z "$offender" ] || break

  case "$seg" in
    *find\ *-type\ [fd]*)
      case "$seg" in
        *-name*|*-path*|*-iname*|*-regex*) ;;  # already narrowed
        *)
          offender="$seg"
          reason="Unfiltered 'find -type f/d' bypasses this repo's Conventions (AGENTS.md: filter shell output before it lands in context). Narrow with -name/-path/-iname/-regex, or pipe through head/grep — rather than listing every match."
          ;;
      esac
      ;;
  esac
  [ -z "$offender" ] || break
done
IFS=$old_ifs

if [ -n "$offender" ]; then
  jq -n --arg reason "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $reason}}'
fi
exit 0
