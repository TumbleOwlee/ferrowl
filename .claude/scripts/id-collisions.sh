#!/bin/sh
# Fixed spec IDs this branch defines that the target branch now defines too —
# two runs (two slices of an epic, two developers) both took the next free
# ID. Run at gate 4, after `git -C <worktree> fetch origin main`.
#
#   sh .claude/scripts/id-collisions.sh <worktree> [<ref>]    # <ref> defaults to origin/main
#
# A definition is a `**<PREFIX>-<R|E>-<n>**` at the start of a line in
# docs/specs/. Prints `ids=clean`, or `ids=collide <ID>,<ID>` and exits 1.
[ $# -ge 1 ] && [ $# -le 2 ] || { echo "usage: $0 <worktree> [<ref>]" >&2; exit 2; }
wt=$1; ref=${2:-origin/main}
base=$(git -C "$wt" merge-base HEAD "$ref") || { echo "ids=error reason=no-merge-base"; exit 2; }
added=$(git -C "$wt" diff "$base" HEAD -- docs/specs | sed -n 's/^+\*\*\([A-Z][A-Z]-[RE]-[0-9][0-9]*\)\*\*.*/\1/p' | sort -u)
hits=
for id in $added; do
    git -C "$wt" grep -q "^\*\*$id\*\*" "$base" -- docs/specs && continue
    git -C "$wt" grep -q "^\*\*$id\*\*" "$ref" -- docs/specs && hits="$hits${hits:+,}$id"
done
if [ -n "$hits" ]; then echo "ids=collide $hits"; exit 1; fi
echo "ids=clean"
