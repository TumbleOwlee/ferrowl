#!/bin/sh
# Append one line of run metrics to .claude/tasks/metrics.log (local,
# gitignored, never cleaned up) and print it. Run at the end of a run, before
# run-cleanup.sh deletes the cards it reads.
#
#   sh .claude/scripts/metrics.sh <slug>
#
# Prints: <slug> start=<ts> end=<ts> first-red=<ts|-> stages=<n> stops=<n>
#         question=<n> approval=<n> stage=<n> blocked=<n> replans=<n> lane=<fast|full|epic|->
# start = the earliest log line on any card of the run, first-red = the
# earliest `test-red` on a stage card, stops = the parent card's `stop kind=`
# lines. METRICS_NOW overrides `end` (tests). Exit 1 when no parent card.
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
line="$line replans=$(count 'replan from=') lane=${lane:--}"
echo "$line" >> "$board/metrics.log"
echo "$line"
