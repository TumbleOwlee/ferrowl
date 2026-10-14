#!/bin/sh
# Read one frontmatter field, or the last log value of one key, from a run's
# parent card, wherever on the board it sits.
#
#   sh .claude/scripts/card-field.sh <slug> <field>         # frontmatter: issue, pr, lane, mode, …
#   sh .claude/scripts/card-field.sh <slug> log:<key>       # last `<key>=<value>` token in the log
#   sh .claude/scripts/card-field.sh <slug> pass-sha        # sha= of the last `gauntlet=pass` log line
#   sh .claude/scripts/card-field.sh <slug> path            # the card's path
#
# Prints the value, or nothing (exit 0) when the card or the field is absent;
# exit 2 on usage.
[ $# -eq 2 ] || { echo "usage: $0 <slug> <field | log:<key> | path>" >&2; exit 2; }
slug=$1; want=$2
board=${CLAUDE_PROJECT_DIR:-.}/.claude/tasks
card=
for d in open inprogress inreview done; do
    [ -f "$board/$d/$slug.md" ] && { card="$board/$d/$slug.md"; break; }
done
[ -n "$card" ] || exit 0
case $want in
    path) printf '%s\n' "$card" ;;
    pass-sha) awk '/^---$/ { fm = !fm; next } !fm && / gauntlet=pass/ { for (i = 2; i <= NF; i++) if (index($i, "sha=") == 1) v = substr($i, 5) } END { if (v != "") print v }' "$card" ;;
    log:*) key=${want#log:}
        awk -v k="$key" '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, k "=") == 1) v = substr($i, length(k) + 2) } END { if (v != "") print v }' "$card" ;;
    *) awk -v k="$want" '/^---$/ { if (fm) exit; fm = 1; next } fm && index($0, k ":") == 1 { sub("^" k ":[ ]*", ""); print; exit }' "$card" ;;
esac
