#!/bin/sh
# Recreate a run's lost board from its issue, its draft PR and git, so the run
# resumes through the usual reconcile (AGENTS.workflow.md `### Resume an
# interrupted run`). plan.md is never rebuilt.
#
#   sh .claude/scripts/run-rebuild.sh <issue> <pr>
#
# Reads issue-view.sh and pr-view.sh (siblings, stubbable), the PR's head
# branch (`gh api`), `git diff main...origin/<branch> -- docs/specs`.
# Writes under .claude/tasks/:
#   inprogress/<slug>.md      parent card: issue, pr, branch, lane from the draft status line, gate dates = today, `rebuild` + `pr=draft` log lines
#   open|done/<slug>.s<n>.md  one card per `## Stages` line carrying `<!-- s<n> [blocked-by …] -->`; ticked → done with `commit=<sha>`
#   artifacts/<slug>/         issue.md, spec-diff.md (scenarios from the issue, entries from the branch diff), plan.summary.md,
#                             and from the PR's `## Architecture`: architecture.summary.md and architecture.md (`## Components`)
# and re-adds the worktree .claude/worktrees/<slug> on the branch.
# Prints `rebuild=done slug=<slug> stages=<n> next=<s<n> | none>`, or
# `rebuild=refused reason=<no-pr|no-issue|no-stages|no-branch|board-exists|sha|worktree> [sha=<sha>]` (exit 1) with nothing written.
[ $# -eq 2 ] || { echo "usage: $0 <issue> <pr>" >&2; exit 2; }
issue=$1 pr=$2
root=${CLAUDE_PROJECT_DIR:-.}
board=$root/.claude/tasks
here=$(dirname -- "$0")
refuse() { echo "rebuild=refused reason=$1${2:+ $2}"; exit 1; }
work=$(mktemp -d) || exit 1
trap 'rm -rf "$work"' EXIT INT TERM

body() { awk '/^by / { on = 1; next } /^--- [0-9]+ comment/ { exit } on' | sed -e :a -e '/^\n*$/{ $d; N; ba' -e '}' | sed '1{/^$/d;}'; }
# section <heading> — lines under `## <heading>` up to the next `## ` or `Closes #`
section() { awk -v h="## $1" '$0 == h { on = 1; next } on && (/^## / || /^Closes #/) { exit } on'; }

pv=$(bash "$here/pr-view.sh" "$pr" 2>/dev/null) || refuse no-pr
printf '%s\n' "$pv" | body > "$work/pr"
grep -q '^## Stages$' "$work/pr" || refuse no-stages

branch=$(gh api "repos/{owner}/{repo}/pulls/$pr" --jq .head.ref 2>/dev/null)
[ -n "$branch" ] || refuse no-branch
slug=${branch#*/}
for c in "$board"/*/"$slug".md "$board"/*/"$slug".s[0-9]*.md "$board/artifacts/$slug"; do [ ! -e "$c" ] || refuse board-exists; done
git -C "$root" fetch -q origin "$branch" 2>/dev/null || refuse no-branch

iv=$(bash "$here/issue-view.sh" "$issue" 2>/dev/null) || refuse no-issue
printf '%s\n' "$iv" | body > "$work/issue"
title=$(printf '%s\n' "$iv" | sed -n '1s/^#[0-9]* //; 1s/ \[[A-Z]*\]$//p')

section Stages < "$work/pr" | awk '
    /^- \[[x ]\] / && match($0, /<!-- s[0-9]+( blocked-by [s0-9,]+)? -->/) {
        n = split(substr($0, RSTART + 5, RLENGTH - 9), w, " ")
        id = w[1]; bb = (w[2] == "blocked-by") ? w[3] : ""
        t = substr($0, 7, RSTART - 7); sub(/ +$/, "", t)
        k = index(t, " — "); if (k) t = substr(t, 1, k - 1)
        sha = ""
        if (match(t, / \([0-9a-f]+\)$/)) { sha = substr(t, RSTART + 2, RLENGTH - 3); t = substr(t, 1, RSTART - 1) }
        printf "%s\t%s\t%s\t%s\t%s\n", id, substr($0, 4, 1) == "x" ? "x" : "-", bb == "" ? "-" : bb, sha == "" ? "-" : sha, t
        cur = id; next
    }
    /^  - beyond unit tests: / { print "V\t" cur "\t" substr($0, 24); next }
    /^    - / { print "S\t" cur "\t" substr($0, 7) }
' > "$work/stages"
awk -F'\t' '$1 != "V" && $1 != "S"' "$work/stages" > "$work/cards"
[ -s "$work/cards" ] || refuse no-stages

tab=$(printf '\t')
while IFS=$tab read -r sid dn bb sha st; do
    [ "$dn" = x ] || continue
    git -C "$root" cat-file -e "$sha^{commit}" 2>/dev/null || refuse sha "sha=$sha"
done < "$work/cards"

wt=$root/.claude/worktrees/$slug
if [ ! -d "$wt" ]; then
    git -C "$root" worktree prune
    if git -C "$root" rev-parse --verify -q "refs/heads/$branch" >/dev/null; then git -C "$root" worktree add -q "$wt" "$branch" || refuse worktree
    else git -C "$root" worktree add -q --track -b "$branch" "$wt" "origin/$branch" || refuse worktree; fi
fi
lane=$(sed -n '1,/./{/./s/.*lane \([a-z]*\).*/\1/p;}' "$work/pr")
today=$(date -u +%F)
mkdir -p "$board/inprogress" "$board/open" "$board/done" "$board/artifacts/$slug"
cat > "$board/inprogress/$slug.md" <<CARD
---
id: $slug
branch: $branch
lane: $lane
issue: $issue
pr: $pr
gate1: $today
arch: $today
gate2: $today
artifacts: .claude/tasks/artifacts/$slug
---
CARD
log() { CLAUDE_PROJECT_DIR=$root sh "$here/card-log.sh" "$@"; }
log "$slug" rebuild reason=board-missing
log "$slug" pr=draft

next=none count=0
while IFS=$tab read -r sid dn bb sha st; do
    count=$((count + 1))
    deps=
    if [ "$bb" != - ]; then
        for d in $(echo "$bb" | tr ',' ' '); do deps="${deps:+$deps, }$slug.$d"; done
    fi
    dir=open; [ "$dn" != x ] || dir=done
    cat > "$board/$dir/$slug.$sid.md" <<CARD
---
id: $slug.$sid
parent: $slug
title: $st
blocked-by: [$deps]
files: []
branch: $branch
worktree: .claude/worktrees/$slug
---
CARD
    if [ "$dn" = x ]; then log "$slug.$sid" "commit=$sha"
    elif [ "$next" = none ]; then next=$sid; fi
done < "$work/cards"

art=$board/artifacts/$slug
{ echo "# $title"; echo; echo "## Goal"; section Goal < "$work/issue" | sed -e :a -e '/^\n*$/{ $d; N; ba' -e '}'
  if grep -q '^## Out of scope$' "$work/issue"; then echo; echo "## Out of scope"; section 'Out of scope' < "$work/issue" | sed -e :a -e '/^\n*$/{ $d; N; ba' -e '}'; fi
} > "$art/issue.md"

{
    echo "# $slug — spec diff (rev 1)"; echo; echo "## Summary"
    echo "Goal: $(section Goal < "$work/issue" | sed -n '/./{p;q;}')"
    section Scenarios < "$work/issue" | awk '
        /^- \*\*S[0-9]+\*\* — / { t = $0; sub(/^- \*\*/, "", t); sub(/\*\* — /, " — ", t); print ""; print "## " t; next }
        /^  - / { print substr($0, 5) }'
    git -C "$root" diff --name-only "main...origin/$branch" -- docs/specs | while read -r f; do
        git -C "$root" diff -U0 "main...origin/$branch" -- "$f" | awk -F'\t' '
            function entry(l,   cells, n, i, t, len) {
                sub(/^- /, "", l)
                if (match(l, /^\*\*[A-Z][A-Z]-[RE]-[0-9]+\*\* — /)) { len = RLENGTH; id = l; sub(/^\*\*/, "", id); sub(/\*\*.*/, "", id); t = substr(l, len + 1); return id "\t" t }
                if (match(l, /^\| \*\*[A-Z][A-Z]-[RE]-[0-9]+\*\* \|/)) {
                    n = split(l, cells, "|"); id = cells[2]; gsub(/[ *]/, "", id); t = ""
                    for (i = 3; i < n; i++) { c = cells[i]; gsub(/^ +| +$/, "", c); t = t (t == "" ? "" : " — ") c }
                    return id "\t" t
                }
                return ""
            }
            /^\+\+\+|^---/ { next }
            /^[+-]/ { e = entry(substr($0, 2)); if (e == "") next; split(e, p, "\t"); id = p[1]
                if (!(id in seen)) { seen[id] = 1; order[++n] = id }
                if (substr($0, 1, 1) == "+") plus[id] = p[2]; else minus[id] = p[2] }
            END { for (i = 1; i <= n; i++) { id = order[i]
                if (id in plus && id in minus) print id "\tchanged\t" plus[id]
                else if (id in plus) print id "\tnew\t" plus[id]
                else print id "\tdropped\t" minus[id] } }' | while IFS=$tab read -r eid kind text; do
            ref=origin/$branch; [ "$kind" != dropped ] || ref=main
            h=$(git -C "$root" show "$ref:$f" | awk -v id="$eid" '/^## / { h = $0 } index($0, "**" id "**") { print h; exit }')
            base=$(basename "$f" .md)
            echo; echo "## $eid — $kind · $base \`$h\`"
            if [ "$kind" = dropped ]; then echo "- removed on the branch"; else echo "$text"; fi
        done
    done
} > "$art/spec-diff.md"

section Architecture < "$work/pr" > "$work/arch"
if [ -s "$work/arch" ]; then
    {
        echo "# $slug — architecture summary (rebuilt)"; echo
        echo "Decide: $(sed -n '/./{p;q;}' "$work/arch")"
        if grep -q '^Public surface:' "$work/arch"; then echo; awk '/^Public surface:/ { on = 1; print; next } on && /^- / { print; next } on { exit }' "$work/arch"; fi
    } > "$art/architecture.summary.md"
    if grep -q '^|' "$work/arch"; then
        { echo "# $slug — architecture (rebuilt)"; echo; echo "## Components"; echo; grep '^|' "$work/arch"; } > "$art/architecture.md"
    fi
fi

{
    echo "# $slug — plan summary (rebuilt)"; echo
    echo "| Stage | What | Pins | Files | Stop |"; echo "|---|---|---|---|---|"
    while IFS=$tab read -r sid dn bb sha st; do echo "| $sid | $st | — | — | — |"; done < "$work/cards"
    if grep -q "^V$tab" "$work/stages"; then
        echo; echo "Verification beyond unit tests:"
        while IFS=$tab read -r sid dn bb sha st; do
            awk -F'\t' -v s="$sid" '$2 == s { if ($1 == "V") print "- " s " — " $3; else print "  - " $3 }' "$work/stages"
        done < "$work/cards"
    fi
} > "$art/plan.summary.md"

echo "rebuild=done slug=$slug stages=$count next=$next"
