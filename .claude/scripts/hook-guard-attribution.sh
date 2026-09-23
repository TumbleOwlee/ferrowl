#!/bin/sh
# PreToolUse guard on the Bash tool: denies a git commit / gh pr create /
# gh pr edit / gh pr comment / gh issue create / gh issue comment that
# carries a tool-attribution trailer (Co-Authored-By, "Generated with",
# ...). CONTRIBUTING.md and .claude/AGENTS.workflow.md both forbid these in
# commits, PR bodies, issues, and comments — this stops the command before
# it runs instead of relying on the agent having read that prose first.
#
# Deliberately does NOT early-exit on a heredoc (`<<`) the way
# hook-guard-shell.sh does: a heredoc is exactly how commit/PR bodies are
# usually passed, so the trailer text lives inside it. A body passed via
# --body-file / --file / -F is scanned too: the file is read if it exists.
#
# Reads a PreToolUse hook payload on stdin, writes a deny-decision JSON
# object on stdout when it blocks, nothing when it doesn't.
set -eu

input=$(cat)
name=$(printf '%s' "$input" | jq -r '.tool_name // empty')
[ "$name" = "Bash" ] || exit 0

cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
[ -n "$cmd" ] || exit 0

case "$cmd" in
  *git\ commit*|*gh\ pr\ create*|*gh\ pr\ edit*|*gh\ pr\ comment*|*gh\ issue\ create*|*gh\ issue\ comment*) ;;
  *) exit 0 ;;
esac

text=$cmd
# --body-file <path> / --file <path> / -F <path>: include the file's content.
for f in $(printf '%s' "$cmd" | sed -n 's/.*\(--body-file\|--file\|-F\)[= ]\{1,\}\([^ ]*\).*/\2/p'); do
  [ -f "$f" ] && text="$text
$(cat "$f")"
done

lower=$(printf '%s' "$text" | tr '[:upper:]' '[:lower:]')

case "$lower" in
  *co-authored-by*|*generated\ with*)
    reason="Command carries a tool-attribution trailer (Co-Authored-By / \"Generated with\"). This repo forbids attribution trailers in commit messages, PR bodies, issues, and comments (CONTRIBUTING.md, .claude/AGENTS.workflow.md). Remove the trailer and retry."
    jq -n --arg reason "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $reason}}'
    ;;
esac
exit 0
