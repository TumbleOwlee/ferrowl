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
# stays for the PR), every card of the run and .claude/tasks/artifacts/<slug>/.
# Prints `cleanup=done` or `cleanup=refused reason=<…>` (exit 1).
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
for d in open inprogress inreview done; do
    rm -f "$board/$d/$slug.md" "$board/$d/$slug".s[0-9]*.md
done
rm -rf "${board:?}/artifacts/$slug"
echo "cleanup=done"
