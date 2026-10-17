#!/bin/sh
# Next step of the run after an agent's status line, for the gate 2, re-plan,
# branch-review, gate 4 and CI decisions: logs the events the outcome implies
# and prints the one next action.
#
#   sh .claude/scripts/next-step.sh <slug> <at> <status line…>
#
# at: plan (planner, plan reviewer, spec-author's issue answer), replan
# (re-plan: planner, plan reviewer), branch (a review in scope branch), pr
# (spec-author at gate 4), ci (ci=<exit code of gh pr checks>). The status line
# may be one argument or several; status= count= user= issue= ci= are read,
# other tokens ignored.
#
# Window: plan reads the parent's whole log; replan and branch the lines after
# the last `replan from=`. Round r = 1 + the window's plan-review= lines.
# Needs user = review.verdict.md has a `Needs user` item. Outgrown = more than
# 2 plan.summary.md rows from s1 on. Rules, first match:
#   plan,replan,pr  question                   relay
#   plan,replan     arch-gap | split | behavior-change   the same word
#   plan,replan     ready                      follow-up (last plan-review=findings) | review
#   plan,replan     findings, r <= 2           logs plan-review=findings; revise
#   plan            clean | findings r >= 3    logs plan-review=; issue
#   replan          clean | findings r >= 3    logs plan-review=; notice | stop (re-plan decision)
#   plan            new | reuse                stop | notice (gate 2 decision)
#   branch          findings count > 0         logs review=<n>-open; fix
#   branch          clean | findings count 0   logs review=<n>-resolved | review=clean;
#                                              stage-stop (open stage) | stop (Needs user, full) | pr
#   pr              ready                      stop (gate 4)
#   ci              ci=0 | ci=<n>              logs ci=; done | fix
# Gate 2 decision: stop reason= lane (not fast), no-arch-stop (arch: not a
# date), needs-user, outgrown, reuse; else notice. Re-plan decision: notice
# (logs replan=pre-authorized) when the last stop before the last replan was
# gate4's, no Needs user, the architecture summary not newer than arch-rev=,
# and not (fast and outgrown); else stop reason= not-gate4, needs-user,
# arch-moved, outgrown.
#
# Prints, in order: `logged: <event and keys>` per event logged,
# `next=<word>[ reason=<word>]`, `run: <command>` when there is one,
# `do: <one line>`, `see: <heading of AGENTS.workflow.md>`.
# Exit 0; 1 `next-step=card` (no parent card); 2 usage, or
# `next-step=unknown at=<at> status=<token>` for a token `at` does not take.
# CARD_LOG_NOW passes through to card-log.sh.
set -u
usage() { echo "usage: $0 <slug> plan|replan|branch|pr|ci <status line…>" >&2; exit 2; }
[ $# -ge 3 ] || usage
slug=$1 at=$2; shift 2
case $at in plan|replan|branch|pr|ci) ;; *) usage ;; esac
status= count= user= issue= ci=
for a in "$@"; do for t in $a; do
    case $t in
        status=*) status=${t#status=} ;; count=*) count=${t#count=} ;; user=*) user=${t#user=} ;;
        issue=*) issue=${t#issue=} ;; ci=*) ci=${t#ci=} ;;
    esac
done; done
if [ "$at" = ci ]; then [ -n "$ci" ] || usage; else [ -n "$status" ] || usage; fi
here=$(dirname -- "$0")

if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then root=$CLAUDE_PROJECT_DIR
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "next-step=card"; exit 1; }
    root=$(cd "$common/.." && pwd)
fi
export CLAUDE_PROJECT_DIR=$root
board=$root/.claude/tasks
art=$board/artifacts/$slug
parent=$(ls "$board"/*/"$slug".md 2>/dev/null | head -1)
[ -n "$parent" ] || { echo "next-step=card"; exit 1; }

field() { sh "$here/card-field.sh" "$slug" "$1"; }
log() { sh "$here/card-log.sh" "$slug" "$@" >/dev/null && echo "logged: $*"; }
out() { echo "next=$1${2:+ reason=$2}"; }
run_() { echo "run: $*"; }
do_() { echo "do: $*"; }
see() { echo "see: $1"; }
unknown() { echo "next-step=unknown at=$at status=${status:-ci=$ci}"; exit 2; }
needs_user() { [ -f "$art/review.verdict.md" ] && awk '/^Needs user/ { on = 1; next } on && /^- / { if ($0 != "- none") f = 1; next } on { on = 0 } END { exit !f }' "$art/review.verdict.md"; }
rev() { [ -f "$1" ] && sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$1"; }
rows() { [ -f "$art/plan.summary.md" ] && awk -F'|' '/^\| s[1-9][0-9]* /' "$art/plan.summary.md" | grep -c .; }
outgrown() { [ "$(rows || true)" -gt 2 ] 2>/dev/null; }
window() { awk -v w="$1" '/^---$/ { fm = !fm; next } !fm { if (w && $2 == "replan" && index($3, "from=") == 1) { buf = ""; next } buf = buf $0 "\n" } END { printf "%s", buf }' "$parent"; }
gate_see() { if [ "$at" = replan ]; then see '#### Re-plan'; else see '### Gate 2 — plan and tracking issue'; fi; }

case $at in plan) win=$(window 0) ;; *) win=$(window 1) ;; esac
nplan=$(printf '%s\n' "$win" | awk '$2 ~ /^plan-review=/ { n++ } END { print n + 0 }')
lastplan=$(printf '%s\n' "$win" | awk '$2 ~ /^plan-review=/ { v = $2 } END { print v }')
r=$((nplan + 1))
count=${count:-0}

gate2_decision() {
    id=${issue:-new}
    reason=
    if [ "$(field lane)" != fast ]; then reason=lane
    else
        case $(field arch) in [0-9][0-9][0-9][0-9]-*) ;; *) reason=no-arch-stop ;; esac
        if [ -z "$reason" ]; then
            if needs_user; then reason=needs-user
            elif outgrown; then reason=outgrown
            elif [ "$status" = reuse ]; then reason=reuse; fi
        fi
    fi
    if [ -n "$reason" ]; then
        out stop "$reason"; run_ sh .claude/scripts/prompt-render.sh gate2 "$slug" "issue=$id"
        do_ "show the gate 2 stop and wait for the user's approval"
    else
        out notice; run_ sh .claude/scripts/prompt-render.sh gate2-notice "$slug" issue=new
        do_ "show the notice without stopping, then act on approval as if given"
    fi
    see '### Gate 2 — plan and tracking issue'
}

replan_decision() {
    from=$(awk '/^---$/ { fm = !fm; next } !fm && $2 == "replan" { for (i = 3; i <= NF; i++) if (index($i, "from=") == 1) v = substr($i, 6) } END { print v }' "$parent")
    laststop=$(awk '/^---$/ { fm = !fm; next } !fm { if ($2 == "replan" && index($3, "from=") == 1) p = s; if ($2 == "stop") s = $0 } END { print p }' "$parent")
    reason=
    case $laststop in *' gate=gate4'*) ;; *) reason=not-gate4 ;; esac
    if [ -z "$reason" ]; then
        ar=$(rev "$art/architecture.summary.md"); archrev=$(field log:arch-rev)
        if needs_user; then reason=needs-user
        elif [ -n "$ar" ] && [ "$ar" -gt "${archrev:-0}" ] 2>/dev/null; then reason=arch-moved
        elif [ "$(field lane)" = fast ] && outgrown; then reason=outgrown; fi
    fi
    if [ -z "$reason" ]; then
        log replan=pre-authorized; out notice; run_ sh .claude/scripts/prompt-render.sh replan-notice "$slug" "from=$from"
        do_ "show the notice without stopping, then continue with the re-planned stages"
    else
        out stop "$reason"; run_ sh .claude/scripts/prompt-render.sh replan "$slug" "from=$from"
        do_ "show the re-plan stop and wait for the user's approval"
    fi
    see '#### Re-plan'
}

open_stage() {
    for c in "$board"/open/"$slug".s[0-9]*.md "$board"/inprogress/"$slug".s[0-9]*.md "$board"/inreview/"$slug".s[0-9]*.md; do
        [ -f "$c" ] || continue; n=${c##*.s}; echo "${n%.md}"
    done | sort -n | head -1
}

case "$at:$status" in
    plan:question|replan:question|pr:question)
        out relay; do_ "relay the question file to the user and resume the agent with the answers"; see '### Relayed question' ;;
    plan:arch-gap|replan:arch-gap)
        out arch-gap; do_ "spawn the architect, then resume the planner with its summary"; gate_see ;;
    plan:split|replan:split)
        out split; do_ "handle the split request"; see '#### Split' ;;
    plan:behavior-change|replan:behavior-change)
        out behavior-change; do_ "the plan found a behavior change in a non-behavior run: back to the spec author"; see '### Principles — implementation' ;;
    plan:ready|replan:ready)
        case $lastplan in
            plan-review=findings*) out follow-up; do_ "spawn the plan reviewer again for a follow-up round" ;;
            *) out review; do_ "spawn the plan reviewer" ;;
        esac
        gate_see ;;
    plan:findings|replan:findings)
        log "plan-review=findings" "count=$count" "round=$r"
        if [ "$r" -le 2 ]; then
            out revise; run_ cp ".claude/tasks/artifacts/$slug/plan.md" ".claude/tasks/artifacts/$slug/plan.prev.md"
            do_ "resume the planner with the review file"; gate_see
        else
            if [ "$at" = plan ]; then
                out issue; do_ "ask the spec author for the tracking issue"; see '### Gate 2 — plan and tracking issue'
            else replan_decision; fi
        fi ;;
    plan:clean|replan:clean)
        log "plan-review=clean" "round=$r"
        if [ "$at" = plan ]; then
            out issue; do_ "ask the spec author for the tracking issue"; see '### Gate 2 — plan and tracking issue'
        else replan_decision; fi ;;
    plan:new|plan:reuse) gate2_decision ;;
    branch:findings)
        if [ "$count" -gt 0 ] 2>/dev/null; then
            log "review=$count-open" scope=branch
            out fix; os=$(open_stage)
            if [ -n "$os" ]; then
                do_ "resume the implementer on the findings, then the gauntlet and the same reviewer's follow-up"; see '#### Per-stage review'
            else
                do_ "move the implicated stage to inprogress, resume its implementer, gauntlet, then a fresh branch review"; see '### Gate 3 — review'
            fi
        else branch_clean=1; fi ;;
    branch:clean) branch_clean=1 ;;
    pr:ready)
        out stop; run_ sh .claude/scripts/prompt-render.sh gate4 "$slug"
        do_ "show the gate 4 stop and wait for the user's approval"; see '### Gate 4 — pull request' ;;
    ci:*)
        if [ "$ci" = 0 ]; then
            log ci=0; out done; run_ sh .claude/scripts/run-cleanup.sh "$slug"
            do_ "clean up the run"; see '### Clean up'
        else
            log "ci=$ci"; out fix; run_ bash .claude/scripts/failed-workflow.sh "$(field branch)"
            do_ "read the failed workflow and fix it"; see '### Gate 4 — pull request'
        fi ;;
    *) unknown ;;
esac

if [ "${branch_clean:-}" = 1 ]; then
    first=$(printf '%s\n' "$win" | awk '$2 == "review=clean" || $2 ~ /^review=[0-9]+-resolved$/ { v = "" } v == "" && $2 ~ /^review=[0-9]+-open$/ { v = $2; sub(/^review=/, "", v); sub(/-open$/, "", v) } END { print v }')
    if [ -n "$first" ]; then log "review=$first-resolved"; else log review=clean; fi
    os=$(open_stage)
    if [ -n "$os" ]; then
        out stage-stop; run_ sh .claude/scripts/stage-stop.sh "$slug" "s${os#s}" 'threads=<n>'
        do_ "run the stage stop decision with the PR thread count"; see '#### Stage stop'
    elif needs_user && [ "$(field lane)" = full ]; then
        out stop; run_ sh .claude/scripts/prompt-render.sh gate3 "$slug"
        do_ "show the gate 3 stop with the open Needs user item"; see '### Gate 3 — review'
    else
        if needs_user; then out pr needs-user; else out pr; fi
        do_ "spawn the spec author for the PR body"; see '### Gate 4 — pull request'
    fi
fi
