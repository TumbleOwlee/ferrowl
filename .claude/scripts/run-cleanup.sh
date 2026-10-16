#!/bin/sh
# Clean up a finished run once its PR is ready and CI is green: the run's
# last step (AGENTS.workflow.md `### Clean up`). The issue and the PR hold
# everything that outlives the run; the merge happens outside the workflow.
#
#   sh .claude/scripts/run-cleanup.sh <slug>
#
# Refuses, changing nothing, while the worktree has uncommitted changes or
# commits its upstream lacks — the PR would not carry them. Else removes the
# worktree .claude/worktrees/<slug>, deletes its local branch (the pushed one
# stays for the PR), then moves every card of the run to
# .claude/tasks/archive/<slug>/cards/ and artifacts/<slug>/ to
# archive/<slug>/artifacts/ (an existing archive/<slug> becomes
# archive/<slug>-<UTC yyyymmddThhmmssZ>); the archive is never pruned.
# Prints `cleanup=done archive=<path | none>` or `cleanup=refused reason=<…>` (exit 1).
[ $# -eq 1 ] || { echo "usage: $0 <slug>" >&2; exit 2; }
slug=$1
root=${CLAUDE_PROJECT_DIR:-.}
wt=$root/.claude/worktrees/$slug
board=$root/.claude/tasks
refuse() { echo "cleanup=refused reason=$1"; exit 1; }

branch=
if [ -d "$wt" ]; then
    branch=$(git -C "$wt" symbolic-ref --quiet --short HEAD) || refuse "detached-head"
    [ -z "$(git -C "$wt" status --porcelain)" ] || refuse "uncommitted-changes"
    git -C "$wt" rev-parse --quiet --verify '@{upstream}' >/dev/null || refuse "no-upstream"
    [ "$(git -C "$wt" rev-list --count '@{upstream}..HEAD')" -eq 0 ] || refuse "unpushed-commits"
    git -C "$root" worktree remove "$wt" || refuse "worktree-remove-failed"
    git -C "$root" branch -D "$branch" >/dev/null || refuse "branch-delete-failed"
fi
set -- "$board"/open/"$slug".md "$board"/open/"$slug".s[0-9]*.md "$board"/inprogress/"$slug".md "$board"/inprogress/"$slug".s[0-9]*.md \
    "$board"/inreview/"$slug".md "$board"/inreview/"$slug".s[0-9]*.md "$board"/done/"$slug".md "$board"/done/"$slug".s[0-9]*.md
have=
for c in "$@"; do [ ! -e "$c" ] || have=1; done
if [ -z "$have" ] && [ ! -d "$board/artifacts/$slug" ]; then echo "cleanup=done archive=none"; exit 0; fi
mkdir -p "$board/archive" 2>/dev/null
dest=$board/archive/$slug
if ! mkdir "$dest" 2>/dev/null; then
    [ -e "$dest" ] || refuse "archive-failed"
    stamp=${CLEANUP_NOW:-$(date -u +%Y%m%dT%H%M%SZ)}; n=0
    dest=$board/archive/$slug-$stamp
    while ! mkdir "$dest" 2>/dev/null; do
        [ -e "$dest" ] || refuse "archive-failed"
        n=$((n + 1)); dest=$board/archive/$slug-$stamp-$n
    done
fi
mkdir "$dest/cards" || refuse "archive-failed"
for c in "$@"; do [ ! -e "$c" ] || mv "$c" "$dest/cards/"; done
[ ! -d "$board/artifacts/$slug" ] || mv "$board/artifacts/$slug" "$dest/artifacts"
echo "cleanup=done archive=$dest"
