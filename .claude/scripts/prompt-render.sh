#!/bin/sh
# Render a stop's whole message — the approval prompt, the question batch or
# the notice line — from the run's files and cards, open the files the user
# approves in the viewer, and log the stop on the parent card. The
# orchestrator relays the output verbatim and composes nothing itself.
#
#   sh .claude/scripts/prompt-render.sh <kind> <slug> [k=v ...]
#
# Approval (`… · <slug> · <files, with rev>`, the quoted lines in fixed
# order, `Approving will:`, `Options:`, `Answer: approve [<n>[: <value>] …] ·
# change <what> · stop`); logs `stop kind=approval gate=<kind>` (a stage
# stop: `stop kind=stage`):
#   gate1    [size=epic reason=<text>] [recommend=full|fast|light]
#   arch     the architecture stop (fast lane: the brief too)
#   epic     the epic stop
#   gate2    [issue=new|<n>]
#   stage    stage=s<n> reason=<stage-stop.sh's reason word> [detail=<text>]
#   replan   from=s<n>
#   gate3
#   gate4    [pr=<n>]
# Question (`Question · <slug> · <where> · <agent> · <n> decisions`, the
# numbered decisions, `Answer: <1: a · 2: b · …> · other <text>`):
#   question where=<gate or s<n>> agent=<agent>     question.md verbatim; logs kind=question
#   blocked  stage=s<n> reason=<text>               logs kind=blocked
#   delta    stage=s<n>                             the verdict's Needs user items; logs kind=approval gate=delta
# Notice (one line, no answer awaited, nothing logged):
#   auto     stage=s<n>                             a stage that auto-continues
#   gate2-notice [issue=new|<n>]                    fast lane: gate 2 pre-authorized
# Record:
#   approved                                        logs `spec-rev=` / `arch-rev=` for the files
#                                                   the last stop showed (its approval marker)
#                                                   and `decisions-seen=` (decisions.md's line count)
#
# Which files a stop shows follows from the cards: spec-diff.md whenever the
# brief is not approved yet (`gate1:` is no date) or its rev moved past the
# last `spec-rev=`; architecture.summary.md at a re-plan only when its rev
# moved past `arch-rev=`; review.verdict.md at gate 2 and a re-plan only with
# a `Needs user` item; at gate 4 in the fast and light lanes. Every stop that
# confirms decisions quotes decisions.md below the last `decisions-seen=`.
# SHOW_FILE_VIEWER passes through to show-file.sh.
# Exit: show-file.sh's (3: a summary over its cap, the agent trims it), 2 usage.
set -u
usage() { echo "usage: $0 <gate1|arch|epic|gate2|stage|replan|gate3|gate4|question|blocked|delta|auto|gate2-notice|approved> <slug> [k=v ...]" >&2; exit 2; }
[ $# -ge 2 ] || usage
kind=$1 slug=$2; shift 2
here=$(cd "$(dirname -- "$0")" && pwd)
for a in "$@"; do case $a in *=*) ;; *) usage ;; esac; done
ARGV=$(printf '%s\n' "$@")
arg() { printf '%s\n' "$ARGV" | sed -n "s/^$1=//p" | tail -1; }

if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then root=$CLAUDE_PROJECT_DIR
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "prompt-render: no repository" >&2; exit 2; }
    root=$(cd "$common/.." && pwd)
fi
export CLAUDE_PROJECT_DIR=$root
board=$root/.claude/tasks
art=$board/artifacts/$slug
parent=$(ls "$board"/*/"$slug".md 2>/dev/null | head -1)
[ -n "$parent" ] || { echo "prompt-render: no card for $slug" >&2; exit 2; }

field() { sh "$here/card-field.sh" "$slug" "$1"; }
rev() { [ -f "$1" ] && sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$1"; }
log() { sh "$here/card-log.sh" "$slug" "$@" >/dev/null; }
has_needs_user() { [ -f "$art/review.verdict.md" ] && awk '/^Needs user/ { on = 1; next } on && /^- / { if ($0 != "- none") f = 1; next } on { on = 0 } END { exit !f }' "$art/review.verdict.md"; }
stages() { for c in "$board"/*/"$slug".s[0-9]*.md; do [ -f "$c" ] || continue; s=${c##*.s}; echo "${s%.md}"; done | sort -n; }

lane=$(field lane); gate1=$(field gate1); branch=$(field branch); issue=$(field issue)
since=$(field log:spec-rev); archrev=$(field log:arch-rev); seen=$(field log:decisions-seen)
brief_pending=0
case $gate1 in [0-9][0-9][0-9][0-9]-*) ;; *) [ -f "$art/spec-diff.md" ] && brief_pending=1 ;; esac
spec_moved() { r=$(rev "$art/spec-diff.md"); [ -n "$r" ] && [ "$r" -gt "${since:-0}" ]; }
[ "$brief_pending" = 1 ] && since=0
want_spec() { [ -f "$art/spec-diff.md" ] && { [ "$brief_pending" = 1 ] || spec_moved; }; }

case $kind in
approved)
    out=approved
    if [ -f "$art/approval" ]; then
        while IFS= read -r f; do
            case $(basename "$f") in
                spec-diff.md) r=$(rev "$f"); [ -n "$r" ] && { log "spec-rev=$r"; out="$out spec-rev=$r"; } ;;
                architecture.summary.md) r=$(rev "$f"); [ -n "$r" ] && { log "arch-rev=$r"; out="$out arch-rev=$r"; } ;;
            esac
        done < "$art/approval"
    fi
    [ -f "$art/decisions.md" ] && { n=$(wc -l < "$art/decisions.md" | tr -d ' '); log "decisions-seen=$n"; out="$out decisions-seen=$n"; }
    echo "$out"; exit 0 ;;
auto)
    sid=$(arg stage); [ -n "$sid" ] || usage
    me=${sid#s}; next=
    for n in $(stages); do [ "$n" -gt "$me" ] && { next=s$n; break; }; done
    pass=$(grep ' gauntlet=pass' "$parent" | tail -1)
    cov=$(printf '%s' "$pass" | sed -n 's/.* cov=\([^ ]*\).*/\1/p'); sha=$(printf '%s' "$pass" | sed -n 's/.* sha=\([^ ]*\).*/\1/p')
    detail=$(sed -n 's/^Spec changes — \(.*\) detail.*/\1/p' "$art/review.verdict.md" 2>/dev/null)
    echo "Landed $sid · $slug · review clean · gauntlet pass ${cov:--} · pushed ${sha:--} · spec detail: ${detail:-none} · next: ${next:-gate 3} (auto-continue — say stop to hold)"
    exit 0 ;;
esac

# --- files the stop shows (viewer) and quotes (prompt lines only)
show= quote=
add() { [ -f "$art/$1" ] && show="$show $art/$1"; }
addq() { [ -f "$art/$1" ] && quote="$quote $art/$1"; }
case $kind in
    gate1) add spec-diff.md; addq decisions.md ;;
    arch) addq decisions.md; want_spec && add spec-diff.md; add architecture.summary.md; add architecture.md ;;
    epic) addq decisions.md; want_spec && add spec-diff.md; add architecture.summary.md; add architecture.md; add slices.md; addq issue.md ;;
    gate2|gate2-notice) addq decisions.md; want_spec && add spec-diff.md; add plan.summary.md; has_needs_user && add review.verdict.md; addq issue.md; addq split-issue.md ;;
    replan)
        addq decisions.md; want_spec && add spec-diff.md
        ar=$(rev "$art/architecture.summary.md"); [ -n "$ar" ] && [ "$ar" -gt "${archrev:-0}" ] && add architecture.summary.md
        add plan.summary.md; has_needs_user && add review.verdict.md ;;
    stage|gate3) add review.verdict.md ;;
    gate4)
        add pr.md
        for f in "$art"/follow-up-*.md; do [ -f "$f" ] && show="$show $f"; done
        case $lane in fast|light) add review.verdict.md ;; esac ;;
    question) addq question.md ;;
    blocked|delta) ;;
    *) usage ;;
esac

viewer_note=
if [ -n "$show" ] && [ "$kind" != gate2-notice ]; then
    # shellcheck disable=SC2086 # artifact paths carry no spaces
    shown=$(SHOW_FILE_LINES=none sh "$here/show-file.sh" $show) || { rc=$?; [ $rc = 3 ] && exit 3; viewer_note="Viewer: failed — open the files manually"; }
    printf '%s\n' "$shown" | grep -q 'open manually' && viewer_note="Viewer: none available — open manually:$(printf '%s\n' "$shown" | sed -n 's/.*open manually: \([^ ]*\).*/ \1/p' | tr -d '\n')"
fi

hdr_files() {
    out=
    for f in $show; do r=$(rev "$f"); out="$out${out:+, }$(basename "$f")${r:+ rev $r}"; done
    printf '%s' "$out"
}

# Quoted lines of every file in one call, reordered into the prompt's fixed block order.
quoted() {
    # shellcheck disable=SC2086
    [ -n "$show$quote$extra" ] || return 0
    { [ -z "$show$quote" ] || DECISIONS_SEEN=${seen:-0} sh "$here/prompt-lines.sh" $show $quote 2>/dev/null
      [ -z "$extra" ] || printf '%s\n' "$extra"; } | awk -v iss="$(arg issue)" '
        function rank(l) {
            if (l ~ /^Goal: /) return 1;      if (l ~ /^Scenarios: /) return 2; if (l ~ /^Entries:/) return 3
            if (l ~ /^Deferred: /) return 4;  if (l ~ /^Size: /) return 5;      if (l ~ /^Architecture: /) return 6
            if (l ~ /^Interfaces: /) return 7; if (l ~ /^Surface:/) return 8;   if (l ~ /^Slices:/) return 9
            if (l ~ /^Plan: /) return 10;     if (l ~ /^Spec since arch: /) return 11; if (l ~ /^Stops:/) return 12
            if (l ~ /^Issue: /) return 13;    if (l ~ /^Split: /) return 14;    if (l ~ /^PR: /) return 15
            if (l ~ /^Follow-up /) return 16; if (l ~ /^Verification:/) return 17; if (l ~ /^Confirm/) return 18
            if (l ~ /^Review: /) return 19;   if (l ~ /^Not checked: /) return 20; if (l ~ /^Spec changes: /) return 22
            if (l ~ /^Spec: /) return 21;     if (l ~ /^Minors /) return 23;    if (l ~ /^Needs user/) return 24
            return 25
        }
        /^(- |  )/ { printf "%02d %04d %04d %s\n", r, b, ++n, $0; next }
        {
            r = rank($0); b++
            if ($0 ~ /^Issue: /) { t = substr($0, 8); $0 = "Issue: " (iss == "" || iss == "new" ? "new" : "reuse #" iss) " — " t }
            if ($0 ~ /^Follow-up /) { if (!fu) { printf "%02d %04d %04d %s\n", r, 0, 0, "Follow-ups:"; fu = 1 } t = $0; sub(/^Follow-up /, "- ", t); printf "%02d %04d %04d %s\n", r, b, ++n, t; next }
            printf "%02d %04d %04d %s\n", r, b, ++n, $0
        }' | sort | cut -d' ' -f4-
}

extra=
[ "$kind" = gate1 ] && [ "$(arg size)" = epic ] && extra="Size: epic recommended — $(arg reason)"
nstages=$(awk -F'|' '/^\| s[0-9]/' "$art/plan.summary.md" 2>/dev/null | grep -c .)
first=$(awk -F'|' '/^\| s[0-9]/ { s = $2; gsub(/ /, "", s); print s; exit }' "$art/plan.summary.md" 2>/dev/null)
entries=$(awk '/^## ([A-Z][A-Z]-[RE]-[0-9]+|[RE][0-9]+) — / && !/ — dropped/' "$art/spec-diff.md" 2>/dev/null | grep -c .)
other_lane() { case $lane in full|light) echo fast ;; *) echo full ;; esac; }

title= will= opts= answer="approve · change <what> · stop"
bp=; [ "$brief_pending" = 1 ] && bp="- approve the brief
"
case $kind in
gate1)
    title="Gate 1"
    if [ "$(arg size)" = epic ]; then
        will="- record the run as an epic (option 1) or a single run in the lane chosen
- hand the brief to the architect"
        opts="1. epic — a shared skeleton, one issue per slice (recommended)
2. single run, full lane
3. single run, fast lane"
    else
        rec=$(arg recommend); rec=${rec:-full}
        will="- settle the lane
- hand the brief to the architect (light lane: to the planner, no architecture step)"
        i=0
        for l in full fast light; do
            [ "$l" = light ] && [ "$entries" -gt 1 ] && continue
            i=$((i + 1)); opts="$opts${opts:+
}$i. $l lane$([ "$l" = "$rec" ] && echo ' (recommended)')"
        done
    fi ;;
arch)
    title="Architecture"
    case $lane in
        fast)
            will="$bp- hand brief and architecture to the planner
- run gate 2 without a stop once the plan review is clean
- next stop: gate 4"
            opts="1. switch to the full lane" ;;
        '')
            will="- hand the architecture to the planner"
            opts="1. full lane
2. fast lane" ;;
        *) will="- hand brief and architecture to the planner" ;;
    esac ;;
epic)
    title="Epic"
    n=$(grep -c '^## Slice [0-9]' "$art/slices.md" 2>/dev/null)
    will="$bp- $(i=$(arg issue); case $i in ''|new) echo "file the epic issue" ;; *) echo "rewrite issue #$i as the epic" ;; esac)
- file $n slice issues in order and link them from the epic
- clean up — no code, no worktree" ;;
gate2)
    title="Gate 2"
    next="per the stage-stop rules"; case $lane in fast|light) next="gate 4" ;; esac
    i=$(arg issue)
    case $i in ''|new) iw="file the issue" ;; *) iw="rewrite issue #$i" ;; esac
    will="$bp- $iw
- create worktree .claude/worktrees/$slug on branch ${branch:-<type>/$slug}
- create $nstages stage cards and spawn the implementer at ${first:-s1}$(case $lane in fast|light) echo ' (commits and pushes on green, no per-stage stop)' ;; esac)
- next stop: $next"
    if [ "$lane" = full ]; then opts="1. also stop after: <stages, e.g. s1,s3>
2. switch to the fast lane"
    else opts="1. switch to the $(other_lane) lane"; fi ;;
stage)
    sid=$(arg stage); title="Stage $sid"
    will="- commit $sid, push, update the draft PR
- spawn the next stage" ;;
replan)
    from=$(arg from); title="Re-plan from $from"
    will="- replace the cards of $from onward
- update the draft PR
- spawn a fresh implementer at $from"
    if [ "$lane" = full ]; then opts="1. also stop after: <stages>
2. switch to the fast lane"
    else opts="1. switch to the $(other_lane) lane"; fi ;;
gate3)
    title="Gate 3"
    will="- hand the branch to spec-author for the PR body (gate 4)" ;;
gate4)
    title="Gate 4"
    pr=$(arg pr); pr=${pr:-$(field pr)}
    k=$(ls "$art"/follow-up-*.md 2>/dev/null | grep -c .)
    will="- push, replace the draft's title and body, mark #${pr:-<pr>} ready$([ "$k" -gt 0 ] && echo "
- file the $k follow-up issues")
- wait for CI, clean up"
    hold="stop (hold, e.g. for a manual run first)"
    if [ "$k" -gt 0 ]; then opts="1. follow-ups: <numbers | none> — file only these (default: all)"; answer="approve [<n>[: <value>] …] · change <what> · $hold"
    else answer="approve · change <what> · $hold"; fi ;;
esac
[ -n "$opts" ] && [ "$kind" != gate4 ] && answer="approve [<n>[: <value>] …] · change <what> · stop"

case $kind in
question)
    where=$(arg where) agent=$(arg agent)
    n=$(grep -c '^## [0-9]*\. ' "$art/question.md" 2>/dev/null)
    log stop kind=question
    echo "Question · $slug · ${where:--} · ${agent:--} · $n decision$([ "$n" = 1 ] || echo s)"
    sed '1{/^# /d}' "$art/question.md"
    echo "Answer: <1: a · 2: b · …> · other <text>"
    exit 0 ;;
blocked)
    sid=$(arg stage) reason=$(arg reason)
    log stop kind=blocked
    echo "Question · $slug · $sid · spec-implementer · 1 decision"
    echo "## 1. $sid is blocked: $reason"
    echo "Options:"
    echo "- a) answer: <text> — relayed to the implementer"
    echo "- b) re-plan — a fresh planner rewrites the stages not yet committed"
    echo "- c) stop — the run halts, the board stays for a resume"
    echo "Answer: <1: a <text> | b | c>"
    exit 0 ;;
delta)
    sid=$(arg stage)
    log stop kind=approval gate=delta
    items=$(awk '/^Needs user/ { on = 1; next } on && /^- / { if ($0 == "- none") next; n++; t = $0; sub(/^- /, "", t); print "## " n ". " t; print "Options:"; next } on && /^  - / { t = $0; sub(/^  - /, "- ", t); print t; next } on { on = 0 }' "$art/review.verdict.md" 2>/dev/null)
    n=$(printf '%s\n' "$items" | grep -c '^## ')
    echo "Question · $slug · delta $sid · spec-reviewer · $n decision$([ "$n" = 1 ] || echo s)"
    printf '%s\n' "$items"
    echo "Answer: <1: a · 2: b · …> · other <text> — keep, revert or split each change"
    exit 0 ;;
gate2-notice)
    dec=$(sed -n 's/^Decide: //p' "$art/plan.summary.md" 2>/dev/null)
    i=$(arg issue); case $i in ''|new) i=new ;; *) i="reuse #$i" ;; esac
    echo "Gate 2 · $slug · $dec · issue $i · starting ${first:-s1} (pre-authorized — say stop to hold)"
    quoted | awk '/^Confirm/ { on = 1; print; next } on && /^- / { print; next } { on = 0 }'
    exit 0 ;;
esac

if [ "$kind" = stage ]; then log stop kind=stage; else log stop kind=approval gate="$kind"; fi
h="$title · $slug"
[ -n "$show" ] && h="$h · $(hdr_files)"
[ -n "$lane" ] && [ "$lane" != full ] && h="$h · $lane lane"
[ "$kind" = stage ] && h="$h · stop: $(arg reason)$( [ -n "$(arg detail)" ] && echo " ($(arg detail))")"
echo "$h"
[ -n "$viewer_note" ] && echo "$viewer_note"
quoted
echo "Approving will:"
printf '%s\n' "$will"
[ -n "$opts" ] && { echo "Options:"; printf '%s\n' "$opts"; }
echo "Answer: $answer"
