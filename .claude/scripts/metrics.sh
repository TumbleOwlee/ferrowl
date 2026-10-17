#!/bin/sh
# Append one line of run metrics to .claude/tasks/metrics.log (local,
# gitignored, never cleaned up) and print it. Run at the end of a run, before
# run-cleanup.sh deletes the cards it reads.
#
#   sh .claude/scripts/metrics.sh <slug>
#
# Prints: <slug> start=<ts> end=<ts> first-red=<ts|-> stages=<n> stops=<n>
#         question=<n> approval=<n> stage=<n> blocked=<n> replans=<n>
#         overturned=<n> reads=<n> lane=<fast|full|light|epic|->
# start = the earliest log line on any card of the run, first-red = the
# earliest `test-red` on a stage card, stops = the parent card's `stop kind=`
# lines, overturned = the sum of the parent's `overturned count=` lines
# (review findings the user decided against), reads = the stage cards'
# `read` lines (implementer reads beyond the plan; a re-plan's replaced
# cards take theirs along — `blocked=` and `replans=` count those).
# METRICS_NOW overrides `end` (tests). Exit 1 when no parent card.
[ $# -eq 1 ] || { echo "usage: $0 <slug>" >&2; exit 2; }
slug=$1
board=${CLAUDE_PROJECT_DIR:-.}/.claude/tasks
parent=
for d in open inprogress inreview done; do
    [ -f "$board/$d/$slug.md" ] && { parent="$board/$d/$slug.md"; break; }
done
[ -n "$parent" ] || { echo "metrics: no card for $slug" >&2; exit 1; }
stages=$(ls "$board"/*/"$slug".s[0-9]*.md 2>/dev/null)
ts='^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2} '
# shellcheck disable=SC2086 # $stages is a newline-separated list of paths without spaces
start=$(grep -hE "$ts" "$parent" $stages | cut -d' ' -f1 | sort | head -1)
# shellcheck disable=SC2086
red=$( [ -n "$stages" ] && grep -hE "$ts"'test-red ' $stages | cut -d' ' -f1 | sort | head -1)
count() { grep -cE "$ts$1" "$parent"; }
lane=$(sh "$(dirname -- "$0")/card-field.sh" "$slug" lane)
[ "$(sh "$(dirname -- "$0")/card-field.sh" "$slug" kind)" = epic ] && lane=epic
line="$slug start=${start:--} end=${METRICS_NOW:-$(date -u +%FT%H:%M)} first-red=${red:--}"
line="$line stages=$(printf '%s' "$stages" | grep -c .) stops=$(count 'stop kind=')"
for k in question approval stage blocked; do line="$line $k=$(count "stop kind=$k( |$)")"; done
ov=$(awk '/ overturned / { for (i = 2; i <= NF; i++) if (index($i, "count=") == 1) n += substr($i, 7) } END { print n + 0 }' "$parent")
# shellcheck disable=SC2086
reads=$( [ -n "$stages" ] && cat $stages | grep -cE "$ts"'read path=')
line="$line replans=$(count 'replan from=') overturned=$ov reads=${reads:-0} lane=${lane:--}"
echo "$line" >> "$board/metrics.log"
echo "$line"
