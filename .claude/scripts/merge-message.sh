#!/bin/sh
# Build the squash-merge commit message for a run, a projection of pr.md.
#
#   sh .claude/scripts/merge-message.sh <slug> > <out>
#   gh pr merge <pr> --squash --subject "$(head -1 <out>)" --body-file <(tail -n +3 <out>)
#
# Line 1: `<PR title> (#<pr>)`. Body, wrapped at 72: pr.md's ## Why lines,
# then its ## Changes bullets. Trailers: `Spec:` with every ID of
# spec-diff.md written out (no ranges, grep-able; several lines when long),
# `Split:`/`Upstream:` from the parent card, `Closes #<issue>` last.
[ $# -eq 1 ] || { echo "usage: $0 <slug>" >&2; exit 2; }
slug=$1
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
[ -f "$a/pr.md" ] || { echo "merge-message: missing $a/pr.md" >&2; exit 1; }
pr=$(field pr); issue=$(field issue); split=$(field split); upstream=$(field upstream)
section() { sed -n "/^## $1\$/,/^## /{ /^## /d; p; }" "$a/pr.md" | sed '/^$/d'; }

printf '%s%s\n\n' "$(head -1 "$a/pr.md" | sed 's/^# //')" "${pr:+ (#$pr)}"
section Why | fold -s -w 72 | sed 's/ *$//'
echo
section Changes | awk '{ if (length($0) <= 72) { print; next } n = split($0, w, " "); line = ""; for (i = 1; i <= n; i++) { cand = (line == "" ? "" : line " ") w[i]; lim = (first_done ? 70 : 72); if (length(cand) > lim && line != "") { print (first_done ? "  " : "") line; line = w[i]; first_done = 1 } else line = cand } print (first_done ? "  " : "") line; first_done = 0 }'
echo

if [ -f "$a/spec-diff.md" ]; then
    grep -o '^## [A-Z][A-Z]-[RE]-[0-9]*' "$a/spec-diff.md" | sed 's/^## //' | awk '
        { if (length(line) + length($0) + 1 > 66) { print "Spec:" line; line = "" } line = line " " $0 }
        END { if (line != "") print "Spec:" line }'
fi
[ -n "$split" ] && printf 'Split: %s\n' "$(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
[ -n "$upstream" ] && printf 'Upstream: %s\n' "$(printf '%s' "$upstream" | tr -d '[]' | sed 's/,/, /g')"
[ -n "$issue" ] && printf 'Closes #%s\n' "$issue"
exit 0
