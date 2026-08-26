#!/bin/sh
# PreToolUse guard on the Bash tool: catches shell-output bypasses of
# AGENTS.md's Conventions ("never read a whole file when only part is
# needed" / "filter shell output before it lands in context") and denies
# them instead of letting the dump land silently. Four shapes:
#   - unpiped `cat` of a markdown file, or of any file over LARGE_LINES lines
#   - unpiped `git show`/`git diff` with no --stat and no pathspec
#   - unpiped `find` with -type f/d and no -name/-path/-iname/-regex
#   - raw `gh issue view` (AGENTS.workflow.md gate 1b: always issue-view.sh)
#   - `git commit` while the checkout is on `main` (branch off main, never
#     commit to it — the safety net for an agent that missed the worktree)
#   - `git push` whose destination is `main`, explicit or (on bare `git
#     push`) implied by the current branch already being `main`
#
# Reads a PreToolUse hook payload on stdin, writes a deny-decision JSON
# object on stdout when it blocks, nothing when it doesn't.
set -eu

LARGE_LINES=80
PROTECTED_BRANCH=main

input=$(cat)
name=$(printf '%s' "$input" | jq -r '.tool_name // empty')
[ "$name" = "Bash" ] || exit 0

cwd=$(printf '%s' "$input" | jq -r '.cwd // "."')
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

  case "$seg" in
    *gh\ issue\ view*)
      offender="$seg"
      reason="Raw 'gh issue view' bypasses this repo's issue-view.sh convention (AGENTS.workflow.md gate 1b: read any issue with 'bash .claude/scripts/issue-view.sh <number|url>', never raw 'gh issue view' — it also sidesteps a GitHub Projects-Classic API bug that crashes the raw form on some repos). Use: bash .claude/scripts/issue-view.sh <number>"
      ;;
  esac
  [ -z "$offender" ] || break

  case "$seg" in
    *git\ commit*)
      case "$seg" in
        *--dry-run*) ;;  # doesn't create a commit
        *)
          dir="$cwd"
          case "$seg" in
            *-C\ *) dir=$(printf '%s' "$seg" | sed -n 's/.*-C[[:space:]]*\([^[:space:]]*\).*/\1/p') ;;
          esac
          branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
          if [ "$branch" = "$PROTECTED_BRANCH" ]; then
            offender="$seg"
            reason="'git commit' while the checkout in $dir is on '$PROTECTED_BRANCH' (AGENTS.workflow.md: branch off main, never commit to main). Create/use a git worktree first: git worktree add .claude/worktrees/<slug> -b <type>/<slug> $PROTECTED_BRANCH — this looks like the worktree step was missed."
          fi
          ;;
      esac
      ;;
  esac
  [ -z "$offender" ] || break

  case "$seg" in
    *git\ push*)
      dir="$cwd"
      case "$seg" in
        *-C\ *) dir=$(printf '%s' "$seg" | sed -n 's/.*-C[[:space:]]*\([^[:space:]]*\).*/\1/p') ;;
      esac
      case "$seg" in
        *" $PROTECTED_BRANCH"|*":$PROTECTED_BRANCH")
          offender="$seg"
          reason="'git push' targeting '$PROTECTED_BRANCH' directly (AGENTS.workflow.md: squash merge to main via PR only, never a direct push). Push the feature branch and open a PR instead."
          ;;
        *)
          branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
          if [ "$branch" = "$PROTECTED_BRANCH" ]; then
            offender="$seg"
            reason="Bare 'git push' while the checkout in $dir is on '$PROTECTED_BRANCH' would push straight to it (AGENTS.workflow.md: branch off main, never commit to main). Create/use a git worktree first: git worktree add .claude/worktrees/<slug> -b <type>/<slug> $PROTECTED_BRANCH — this looks like the worktree step was missed."
          fi
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
