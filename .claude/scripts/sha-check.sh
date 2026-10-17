#!/bin/sh
# Verify an implementer's reported commit before anything is pushed.
#
#   sh .claude/scripts/sha-check.sh <slug> s<n> <sha>
#
# Checks, in order: the sha names a commit in the run's worktree, the run's
# branch (parent card `branch:`) contains it, and the stage card's last
# `commit=` line names the same commit. Prints `sha=ok` (exit 0) or
# `sha=mismatch reason=<card|missing|off-branch|card-commit>` (exit 1).
set -u
[ $# -eq 3 ] || { echo "usage: $0 <slug> s<n> <sha>" >&2; exit 2; }
slug=$1 sid=$2 sha=$3

if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then root=$CLAUDE_PROJECT_DIR
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "sha=mismatch reason=card"; exit 1; }
    root=$(cd "$common/.." && pwd)
fi
board=$root/.claude/tasks
miss() { echo "sha=mismatch reason=$1"; exit 1; }
parent=$(ls "$board"/*/"$slug".md 2>/dev/null)
card=$(ls "$board"/*/"$slug.$sid".md 2>/dev/null)
case $parent$card in *'
'*) miss card ;; esac
[ -n "$parent" ] && [ -n "$card" ] || miss card

fm() { awk -v k="$2" '/^---$/ { if (fm) exit; fm = 1; next } fm && index($0, k ":") == 1 { sub("^" k ":[ ]*", ""); print; exit }' "$1"; }
branch=$(fm "$parent" branch)
wt=$(fm "$card" worktree); wt=${wt:-.claude/worktrees/$slug}
case $wt in /*) ;; *) wt=$root/$wt ;; esac

full=$(git -C "$wt" rev-parse -q --verify "$sha^{commit}" 2>/dev/null) || miss missing
[ -n "$branch" ] && git -C "$wt" branch --contains "$full" 2>/dev/null | sed 's/^[*+ ] //' | grep -qxF "$branch" || miss off-branch
logged=$(awk '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, "commit=") == 1) v = substr($i, 8) } END { print v }' "$card")
[ -n "$logged" ] && [ "$(git -C "$wt" rev-parse -q --verify "$logged^{commit}" 2>/dev/null)" = "$full" ] || miss card-commit
echo "sha=ok"
