#!/bin/sh
# Reconcile a run's cards with git before an interrupted run resumes: card =
# intent, git = fact. Covers the rows git alone answers; the issue, the PR
# and the PR body stay the orchestrator's (they need gh).
#
#   sh .claude/scripts/reconcile.sh <slug>
#
# Rows: worktree (registered with git and present on disk, once `gate2` is
# set), branch (parent `branch:` exists locally), every stage `commit=` sha
# (exists, on the branch), every stage in done/ (has a commit), the last
# parent `gauntlet=pass sha=` (on the branch).
# Prints one `differ row=<row> <detail>` line per disagreement, then
# `reconcile=clean next=<s<n>|none>` (exit 0) or `reconcile=differ n=<k>
# next=…` (exit 1); next = the lowest stage card outside done/.
# Exit 2: usage or no parent card.
set -u
[ $# -eq 1 ] || { echo "usage: $0 <slug>" >&2; exit 2; }
slug=$1
if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then root=$CLAUDE_PROJECT_DIR
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "reconcile: no repository" >&2; exit 2; }
    root=$(cd "$common/.." && pwd)
fi
board=$root/.claude/tasks
parent=$(ls "$board"/*/"$slug".md 2>/dev/null | head -1)
[ -n "$parent" ] || { echo "reconcile: no card for $slug" >&2; exit 2; }

fm() { awk -v k="$2" '/^---$/ { if (fm) exit; fm = 1; next } fm && index($0, k ":") == 1 { sub("^" k ":[ ]*", ""); print; exit }' "$1"; }
lastval() { awk -v k="$2" '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, k "=") == 1) v = substr($i, length(k) + 2) } END { print v }' "$1"; }
n=0
differ() { echo "differ row=$*"; n=$((n + 1)); }
branch=$(fm "$parent" branch)
wt=$root/.claude/worktrees/$slug

if [ -n "$(fm "$parent" gate2)" ]; then
    git -C "$root" worktree list --porcelain 2>/dev/null | grep -qxF "worktree $wt" && [ -d "$wt" ] || differ "worktree $wt missing"
fi
onbranch=0
if [ -n "$branch" ]; then
    if git -C "$root" rev-parse -q --verify "refs/heads/$branch" >/dev/null; then onbranch=1
    else differ "branch $branch missing"; fi
fi
contains() { [ "$onbranch" = 1 ] && git -C "$root" merge-base --is-ancestor "$1" "refs/heads/$branch" 2>/dev/null; }

next=none
for n_ in $(for c in "$board"/*/"$slug".s[0-9]*.md; do [ -f "$c" ] || continue; s=${c##*.s}; echo "${s%.md}"; done | sort -n); do
    c=$(ls "$board"/*/"$slug.s$n_".md | head -1)
    state=$(basename "$(dirname "$c")")
    sha=$(lastval "$c" commit)
    if [ -n "$sha" ]; then
        if ! git -C "$root" rev-parse -q --verify "$sha^{commit}" >/dev/null; then differ "commit s$n_ $sha does not exist"
        elif [ -n "$branch" ] && ! contains "$sha"; then differ "commit s$n_ $sha not on $branch"; fi
    elif [ "$state" = done ]; then differ "done s$n_ has no commit"; fi
    [ "$state" != done ] && [ "$next" = none ] && next=s$n_
done
g=$(awk '/^---$/ { fm = !fm; next } !fm && / gauntlet=pass/ { for (i = 2; i <= NF; i++) if (index($i, "sha=") == 1) v = substr($i, 5) } END { print v }' "$parent")
[ -n "$g" ] && [ -n "$branch" ] && ! contains "$g" && differ "gauntlet $g not on $branch"
if [ "$n" -eq 0 ]; then echo "reconcile=clean next=$next"; else echo "reconcile=differ n=$n next=$next"; exit 1; fi
