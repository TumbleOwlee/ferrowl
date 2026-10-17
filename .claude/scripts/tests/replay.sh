#!/bin/sh
# Replay a scripted run: fake agents answer with fixed status lines and
# commits, the orchestrator's side is the decision scripts it calls in
# AGENTS.workflow.md order. Prints a transcript — every call's output and
# the board after each step — that run.sh compares with
# fixtures/expected/replay-<scenario>.out. Deterministic: fixed clock,
# identity and commit dates, so shas are stable.
#
#   sh .claude/scripts/tests/replay.sh <full|fast-replan|fast-noarch|pr-replan|resume>
set -u
scenario=${1:?usage: replay.sh <full|fast-replan|fast-noarch|pr-replan|resume>}
scripts=$(cd "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d) || exit 1
trap 'rm -rf "$tmp"' EXIT INT TERM
R=$tmp/r
export CLAUDE_PROJECT_DIR=$R CARD_LOG_NOW=2026-10-05T09:00 METRICS_NOW=2026-10-05T18:00 SHOW_FILE_VIEWER=none
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
export GIT_AUTHOR_DATE=2026-10-05T09:00:00Z GIT_COMMITTER_DATE=2026-10-05T09:00:00Z
B=$R/.claude/tasks; W=$R/.claude/worktrees/rp; A=$B/artifacts/rp

step() { printf '\n## %s\n' "$*"; }
# run <script> <args…> — the call and its output, paths shortened
run() { s=$1; shift; echo "\$ $s $*"; sh "$scripts/$s" "$@" 2>&1 | sed "s#$R#<R>#g"; }
board() { echo "board:"; for d in open inprogress inreview done; do for c in "$B/$d"/rp*.md; do [ -f "$c" ] && echo "  $d/$(basename "$c")"; done; done; }
git_() { git -C "$W" -c commit.gpgsign=false "$@"; }

setup() { # <lane> <stages besides s0>
    git init -q -b main "$R"; printf '.claude/\n' > "$R/.gitignore"
    git -C "$R" add .gitignore; git -C "$R" -c commit.gpgsign=false commit -q -m init
    for d in open inprogress inreview done artifacts/rp; do mkdir -p "$B/$d"; done
    printf -- '---\nid: rp\nbranch: feat/rp\nlane: %s\nissue: 7\ngate1: 2026-10-01\narch: 2026-10-02\n---\n' "$1" > "$B/open/rp.md"
    { printf '# rp — plan summary (rev 1)\n\nDecide: %s code stages.\n\n| Stage | What | Pins | Files | Stop |\n|---|---|---|---|---|\n' "$2"
      i=0; while [ "$i" -le "$2" ]; do printf '| s%s | stage %s | R1 (1) | 1 (x) | — |\n' "$i" "$i"; i=$((i + 1)); done
      [ "$2" -ge 2 ] && printf '\nVerification beyond unit tests:\n- s2 — demo TUI\n  - see it\n'; } > "$A/plan.summary.md"
    printf 'Verdict: clean — ready for approval\n\nSpec — 2 of 2 pinned\nNot checked — nothing\n' > "$A/review.verdict.md"
    i=0
    while [ "$i" -le "$2" ]; do
        b="[]"; [ "$i" -gt 0 ] && b="[rp.s$((i - 1))]"
        printf -- '---\nid: rp.s%s\nparent: rp\ntitle: Stage %s\nblocked-by: %s\nfiles: [f%s.txt]\n---\n' "$i" "$i" "$b" "$i" > "$B/open/rp.s$i.md"
        i=$((i + 1))
    done
}

# impl <n> — the fake implementer takes stage n to green: inprogress, a file, inreview
impl() {
    run card-move.sh "rp.s$1" inprogress
    printf 'stage %s\n' "$1" > "$W/f$1.txt"
    run card-log.sh "rp.s$1" green
    run card-move.sh "rp.s$1" inreview
    echo "impl: status=inreview stage=s$1 delta=0"
}
# commit <n> — the fake implementer commits; the orchestrator verifies, gauntlets (stubbed pass) and moves to done
commit() {
    git_ add "f$1.txt"; git_ commit -q -m "s$1"; sha=$(git_ rev-parse --short HEAD)
    run card-log.sh "rp.s$1" "commit=$sha"
    echo "impl: status=committed stage=s$1 sha=$sha delta=0"
    run sha-check.sh rp "s$1" "$sha"
    run card-log.sh rp gauntlet=pass cov=81% "sha=$sha"
    run card-move.sh "rp.s$1" done
}

case $scenario in
full)
    setup full 3
    step "gate 2 not yet approved: the parent cannot start"
    run card-move.sh rp inprogress
    step "gate 2 approved"
    sed -i 's/^arch: .*/&\ngate2: 2026-10-03/' "$B/open/rp.md"
    git -C "$R" worktree add -q "$W" -b feat/rp main
    run card-move.sh rp inprogress
    step "s1 cannot start before s0 is done"
    run card-move.sh rp.s1 inprogress
    step "s0 lands the spec, never stops"
    impl 0; run stage-stop.sh rp s0; commit 0; board
    step "s1 green, clean review: auto-continue"
    impl 1
    step "a done move before the commit is refused"
    run card-move.sh rp.s1 done
    run stage-stop.sh rp s1; commit 1; run prompt-render.sh auto rp stage=s1; board
    step "s2 green: its plan Verification stops it"
    impl 2; run stage-stop.sh rp s2
    run prompt-render.sh stage rp stage=s2 reason=verification "detail=demo TUI"
    echo "user: approve"; run prompt-render.sh approved rp; commit 2
    step "s3 green, clean branch review: commits without a stop, gate 4's stop is its stop"
    impl 3; run stage-stop.sh rp s3; commit 3; run prompt-render.sh auto rp stage=s3
    run card-move.sh rp inreview; board
    step "gate 4 shows the branch verdict"
    printf '# Read register 65535\n\n## Why\nThe last register reads.\n' > "$A/pr.md"
    run prompt-render.sh gate4 rp pr=9
    step "end of run"
    run reconcile.sh rp
    run card-log.sh rp ci=0
    run metrics.sh rp ;;
fast-replan)
    setup fast 2
    sed -i 's/^arch: .*/&\ngate2: 2026-10-03/' "$B/open/rp.md"
    git -C "$R" worktree add -q "$W" -b feat/rp main
    run card-move.sh rp inprogress
    impl 0; commit 0
    step "s1 blocked: a plan reference too thin to act on, the read budget spent"
    run card-move.sh rp.s1 inprogress
    for f in f1.txt f0.txt f1.txt; do run card-log.sh rp.s1 read "path=$f" -- no signature for parse_frame; done
    run card-log.sh rp.s1 read path=src/lib.rs -- still nothing
    echo "impl: status=blocked stage=s1 reason=parse_frame has no signature in the plan"
    run prompt-render.sh blocked rp stage=s1 "reason=parse_frame has no signature in the plan"
    echo "user: 1: b"
    step "re-plan from the lowest stage without a commit"
    run card-log.sh rp replan from=s1
    rm "$B"/*/rp.s1.md "$B"/*/rp.s2.md
    for i in 1 2 3; do
        printf -- '---\nid: rp.s%s\nparent: rp\ntitle: Stage %s\nblocked-by: [rp.s%s]\nfiles: [f%s.txt]\n---\n' "$i" "$i" "$((i - 1))" "$i" > "$B/open/rp.s$i.md"
    done
    printf '# rp — plan summary (rev 2)\n\nDecide: 3 stages.\n' > "$A/plan.summary.md"
    run prompt-render.sh replan rp from=s1
    echo "user: approve"; run prompt-render.sh approved rp
    run card-log.sh rp replan=approved; board
    step "the new stages land on green"
    for i in 1 2 3; do impl "$i"; commit "$i"; done
    board
    run card-log.sh rp ci=0
    run metrics.sh rp ;;
fast-noarch)
    setup fast 1
    sed -i 's/^gate1: .*/gate1: with-plan/; s/^arch: .*/arch: skipped/' "$B/open/rp.md"
    printf '# rp — spec diff (rev 1)\n\n## Summary\nGoal: Register 65535 reads correctly.\n\n## S1 — Read the last register\n\n## R1 — changed · requirements `## Addressing`\nAddresses span 0 to 65535 inclusive.\n' > "$A/spec-diff.md"
    printf '# Read register 65535\n' > "$A/issue.md"
    step "no architecture step: gate 2 is the first stop and approves the brief"
    run prompt-render.sh gate2 rp issue=new
    echo "user: approve"; run prompt-render.sh approved rp
    sed -i 's/^gate1: .*/gate1: 2026-10-03\ngate2: 2026-10-03/' "$B/open/rp.md"
    git -C "$R" worktree add -q "$W" -b feat/rp main
    run card-move.sh rp inprogress
    step "one code stage, committed on green"
    impl 0; commit 0; impl 1; commit 1; board
    step "gate 3's branch review is the only code review; gate 4 shows its verdict"
    run card-move.sh rp inreview
    printf '# Read register 65535\n\n## Why\nThe last register reads.\n' > "$A/pr.md"
    run prompt-render.sh gate4 rp pr=9
    run card-log.sh rp ci=0
    run metrics.sh rp ;;
pr-replan)
    setup full 1
    sed -i 's/^arch: .*/&\ngate2: 2026-10-03/' "$B/open/rp.md"
    git -C "$R" worktree add -q "$W" -b feat/rp main
    run card-move.sh rp inprogress
    impl 0; run stage-stop.sh rp s0; commit 0
    step "s1, the last code stage, clean branch review: commits without a stop"
    impl 1; run stage-stop.sh rp s1; commit 1; run prompt-render.sh auto rp stage=s1
    run card-move.sh rp inreview
    printf '# Read register 65535\n\n## Why\nThe last register reads.\n' > "$A/pr.md"
    step "gate 4 shows the verdict; the user asks for a re-plan"
    run prompt-render.sh gate4 rp pr=9
    echo "user: re-plan: split the parser out"
    step "re-plan from s2, pre-authorized at gate 4: a notice, no stop"
    run card-move.sh rp inprogress
    run card-log.sh rp replan from=s2
    printf -- '---\nid: rp.s2\nparent: rp\ntitle: Stage 2\nblocked-by: [rp.s1]\nfiles: [f2.txt]\n---\n' > "$B/open/rp.s2.md"
    printf '# rp — plan summary (rev 2)\n\nDecide: 1 more stage.\n' > "$A/plan.summary.md"
    run prompt-render.sh replan-notice rp from=s2
    run card-log.sh rp replan=pre-authorized; board
    step "s2 lands on its clean branch review; gate 4 shows verdict, new plan and PR"
    impl 2; run stage-stop.sh rp s2; commit 2; run prompt-render.sh auto rp stage=s2
    run card-move.sh rp inreview
    run prompt-render.sh gate4 rp pr=9
    run card-log.sh rp ci=0
    run metrics.sh rp ;;
resume)
    setup full 2
    sed -i 's/^arch: .*/&\ngate2: 2026-10-03/' "$B/open/rp.md"
    git -C "$R" worktree add -q "$W" -b feat/rp main
    run card-move.sh rp inprogress
    impl 0; commit 0
    step "the session dies after s1's card claims a commit that never landed"
    impl 1; run card-log.sh rp.s1 commit=0badc0d
    board
    step "resume: reconcile before acting"
    run reconcile.sh rp
    run sha-check.sh rp s1 0badc0d
    run card-move.sh rp.s1 done
    step "the worktree is gone as well"
    git -C "$R" worktree remove --force "$W"
    run reconcile.sh rp ;;
*) echo "unknown scenario: $scenario" >&2; exit 2 ;;
esac
