#!/bin/sh
# Decide whether a green, clean-reviewed full-lane stage stops for approval
# or auto-continues, and log the outcome on its card.
#
#   sh .claude/scripts/stage-stop.sh <slug> s<n> [threads=<n>]
#
# Rules, first hit wins:
#   1 needs-user     review.verdict.md has a `Needs user` item
#   2 verification   plan.summary.md's `Verification beyond unit tests:` names s<n>
#   3 pr-threads     threads=<n> (pr-feedback.sh fetch) above 0
#   4 (retired)      the last code stage stops only by another rule; else gate 4's stop is its stop
#   5 plan-marked    the card's frontmatter says `stop: yes`
#   6 diff-size      the worktree's diff from the previous stage's commit (the
#                    branch point for the first stage), untracked files
#                    included, exceeds 400 changed lines or 8 files
#   7 checkpoint     three stages auto-continued since the last stop
# s0 never stops.
#
# Prints `stop=<rule> reason=<word>[ detail=<text>]` or `auto`, and logs
# `stop=<rule>` or `auto=yes` on the stage card through card-log.sh.
# Exit 1: no such card; 2: usage. STAGE_STOP_LINES / STAGE_STOP_FILES
# override the thresholds (tests).
set -u
[ $# -ge 2 ] || { echo "usage: $0 <slug> s<n> [threads=<n>]" >&2; exit 2; }
slug=$1 sid=$2; shift 2
threads=0
for a in "$@"; do case $a in threads=*) threads=${a#threads=} ;; *) echo "usage: $0 <slug> s<n> [threads=<n>]" >&2; exit 2 ;; esac; done
here=$(dirname -- "$0")

if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then root=$CLAUDE_PROJECT_DIR
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "stage-stop=card"; exit 1; }
    root=$(cd "$common/.." && pwd)
fi
board=$root/.claude/tasks
art=$board/artifacts/$slug
card=$(ls "$board"/*/"$slug.$sid".md 2>/dev/null)
case $card in ''|*'
'*) echo "stage-stop=card"; exit 1 ;; esac

fm() { awk -v k="$2" '/^---$/ { if (fm) exit; fm = 1; next } fm && index($0, k ":") == 1 { sub("^" k ":[ ]*", ""); print; exit }' "$1"; }
lastcommit() { awk '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, "commit=") == 1) v = substr($i, 8) } END { print v }' "$1"; }
num() { n=${1##*.s}; echo "${n%.md}"; }

decide() {
    [ "$sid" = s0 ] && { echo auto; return; }
    v=$art/review.verdict.md
    if [ -f "$v" ] && awk '/^Needs user/ { on = 1; next } on && /^- / { if ($0 != "- none") f = 1; next } on { on = 0 } END { exit !f }' "$v"; then
        echo "stop=1 reason=needs-user"; return
    fi
    ps=$art/plan.summary.md
    if [ -f "$ps" ]; then
        what=$(awk -v s="$sid" '/^Verification beyond unit tests:/ { on = 1; next }
            on && /^- / { line = $0; sub(/^- /, "", line); split(line, parts, " — "); n = split(parts[1], ids, /, */)
                for (i = 1; i <= n; i++) if (ids[i] == s) { print parts[2]; exit } next }
            on && /^  - / { next } on { on = 0 }' "$ps")
        [ -n "$what" ] && { echo "stop=2 reason=verification detail=\"$what\""; return; }
    fi
    [ "$threads" -gt 0 ] 2>/dev/null && { echo "stop=3 reason=pr-threads detail=\"$threads threads\""; return; }
    me=${sid#s}
    [ "$(fm "$card" stop)" = yes ] && { echo "stop=5 reason=plan-marked"; return; }

    wt=$(fm "$card" worktree); wt=${wt:-.claude/worktrees/$slug}
    case $wt in /*) ;; *) wt=$root/$wt ;; esac
    base=
    prev=$((me - 1))
    while [ "$prev" -ge 0 ] && [ -z "$base" ]; do
        pc=$(ls "$board"/*/"$slug.s$prev".md 2>/dev/null | head -1)
        [ -n "$pc" ] && base=$(lastcommit "$pc")
        prev=$((prev - 1))
    done
    [ -n "$base" ] || base=$(git -C "$wt" merge-base HEAD main 2>/dev/null)
    if [ -n "$base" ] && git -C "$wt" rev-parse -q --verify "$base^{commit}" >/dev/null 2>&1; then
        set -- $(git -C "$wt" diff --numstat "$base" 2>/dev/null | awk '{ l += $1 + $2; f++ } END { print l + 0, f + 0 }')
        lines=$1 files=$2
        for u in $(git -C "$wt" ls-files --others --exclude-standard 2>/dev/null); do
            lines=$((lines + $(wc -l < "$wt/$u"))); files=$((files + 1))
        done
        if [ "$lines" -gt "${STAGE_STOP_LINES:-400}" ] || [ "$files" -gt "${STAGE_STOP_FILES:-8}" ]; then
            echo "stop=6 reason=diff-size detail=\"$lines lines, $files files\""; return
        fi
    fi

    run=0
    for n in $(for c in "$board"/*/"$slug".s[0-9]*.md; do [ -f "$c" ] && num "$c"; done | sort -n); do
        [ "$n" -ge "$me" ] && break
        [ "$n" = 0 ] && continue
        c=$(ls "$board"/*/"$slug.s$n".md | head -1)
        last=$(grep -oE ' (auto|stop)=' "$c" | tail -1)
        case $last in ' auto=') run=$((run + 1)) ;; ' stop=') run=0 ;; esac
    done
    [ "$run" -ge 3 ] && { echo "stop=7 reason=checkpoint"; return; }
    echo auto
}

out=$(decide)
case $out in
    stop=*) r=${out#stop=}; sh "$here/card-log.sh" "$slug.$sid" "stop=${r%% *}" >/dev/null ;;
    *) sh "$here/card-log.sh" "$slug.$sid" auto=yes >/dev/null ;;
esac
echo "$out"
