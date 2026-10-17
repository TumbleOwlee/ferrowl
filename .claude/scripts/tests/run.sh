#!/bin/sh
# Fixture tests for the workflow scripts in .claude/scripts/. Runs in CI and
# locally; needs sh, awk, sed, git — no gh, no cargo.
#
#   sh .claude/scripts/tests/run.sh            # run every test
#   UPDATE=1 sh .claude/scripts/tests/run.sh   # rewrite expected/*.out from the current scripts
#
# A golden test runs one command against a copy of fixtures/board as a
# project's .claude/tasks/ and compares its stdout plus `exit=<code>` with
# fixtures/expected/<name>.out; review every rewritten file in the diff
# before committing it. Assertion tests check exit codes and deny decisions
# where the exact output is not the contract. replay.sh replays whole runs
# through the decision scripts in workflow order. Scripts that only wrap gh
# (issue-view, pr-view, pr-feedback, failed-workflow) are not covered.
set -u
here=$(cd "$(dirname -- "$0")" && pwd)
scripts=$(dirname -- "$here")
fx=$here/fixtures
tmp=$(mktemp -d) || exit 1
trap 'rm -rf "$tmp"' EXIT INT TERM
pass=0; fail=0

ok() { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1${2:+ — $2}"; }

# Fresh project root with the fixture board, for each test.
project() {
    rm -rf "$tmp/p"; mkdir -p "$tmp/p/.claude"
    cp -R "$fx/board" "$tmp/p/.claude/tasks"
    ln -s "$scripts" "$tmp/p/.claude/scripts"
    echo "$tmp/p"
}

# golden <name> <command…> — stdout and exit code against expected/<name>.out
golden() {
    name=$1; shift
    out=$("$@" 2>/dev/null; echo "exit=$?")
    want=$fx/expected/$name.out
    if [ "${UPDATE:-}" = 1 ]; then printf '%s\n' "$out" > "$want"; ok "$name (updated)"; return; fi
    if [ -f "$want" ] && [ "$out" = "$(cat "$want")" ]; then ok "$name"
    else bad "$name"; printf '%s\n' "$out" | diff "$want" - | sed 's/^/     /' | head -20; fi
}

# hook <name> <script> <command> deny|allow — a PreToolUse payload for Bash
hook() {
    name=$1 script=$2 cmd=$3 want=$4
    payload=$(printf '%s' "$cmd" | awk 'BEGIN { RS = "\0" } { gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); gsub(/\n/, "\\n"); printf "{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"%s\"}}", $0 }')
    got=$(printf '%s' "$payload" | (cd "${HOOK_CWD:-$tmp/p}" && CLAUDE_PROJECT_DIR=${HOOK_CWD:-$tmp/p} sh "$scripts/$script") 2>/dev/null)
    case $got in *'"deny"'*) res=deny ;; *) res=allow ;; esac
    if [ "$res" = "$want" ]; then ok "$name"; else bad "$name" "expected $want, got $res"; fi
}

# --- issue-body.sh
P=$(project)
golden issue-body-demo env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" demo
golden issue-body-nospec env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" nospec
golden issue-body-violated env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" violated
golden issue-body-missing env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" nosuch

golden issue-body-epic env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" lb
golden issue-body-slice-1 env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" lb --slice 1
golden issue-body-slice-2 env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" lb --slice 2
golden issue-body-slice-3 env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" lb --slice 3
golden issue-body-slice-run env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" sl
golden issue-body-slice-missing env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" lb --slice 9
golden issue-body-mrc env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" mrc

# --status rewrites only line 1 of a filed body
printf '%s\n\n%s\n%s\n' '**Status** planned · **Lane** full' '## Goal' kept > "$tmp/live.md"
printf '\n%s\n\n%s\n%s\n' '**Status** planned · **Lane** full' '## Goal' kept > "$tmp/live-blank.md"
golden issue-body-status env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" demo --status "$tmp/live.md"
golden issue-body-status-blank env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" demo --status "$tmp/live-blank.md"
printf '%s\n\n%s\n%s\n' '**Spec** rev 3' '## Goal' kept > "$tmp/live-spec.md"
printf '%s\n\n%s\n%s\n' '**Part of epic** #40' '## Goal' kept > "$tmp/live-slice.md"
golden issue-body-status-spec env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" demo --status "$tmp/live-spec.md"
golden issue-body-status-slice env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" demo --status "$tmp/live-slice.md"
echo '2026-10-04T10:00 ci=0' >> "$P/.claude/tasks/inprogress/demo.md"
golden issue-body-status-done env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" demo --status "$tmp/live.md"

# the visible text names no stage or agent
refuse() { # <replacement for the Today line> — a copy of demo without a card
    P=$(project); A=$P/.claude/tasks/artifacts
    cp -R "$A/demo" "$A/sref"
    sed -i "s#Today the user restarts the tab by hand.#$1#" "$A/sref/issue.md"
}
refuse 'Today s3 restarts it.'
golden issue-body-refused env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" sref
refuse 'Today the spec-architect restarts it.'
golden issue-body-refused-agent env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" sref
refuse 'Today `s3` restarts it.'
golden issue-body-refused-code env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" sref
refuse 'Today `x` restarts s3. See `y`.'
golden issue-body-refused-between env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" sref
refuse 'Today `docs/s3.md` restarts it.'
env CLAUDE_PROJECT_DIR="$P" sh "$scripts/issue-body.sh" sref >/dev/null 2>&1
[ $? -eq 0 ] && ok issue-body-path-span-passes || bad issue-body-path-span-passes "a file name in a code span is not a stage ref"
P=$(project)

# --- spec-render.sh
SD=$P/.claude/tasks/artifacts/demo/spec-diff.md
for part in scenarios requirements entries changes deferred; do
    golden spec-render-$part sh "$scripts/spec-render.sh" "$SD" $part
done
golden spec-render-entries-only sh "$scripts/spec-render.sh" "$P/.claude/tasks/artifacts/lb/spec-diff.md" entries --only "R1" --first 1
golden spec-render-usage sh "$scripts/spec-render.sh"
golden spec-render-missing sh "$scripts/spec-render.sh" "$P/nosuch.md" scenarios

# --- pr-body.sh
golden pr-body-draft env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" demo draft
golden pr-body-full env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" demo full
golden pr-body-mrcpr env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" mrcpr full
golden pr-body-mrcpr-draft env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" mrcpr draft
cp -R "$P/.claude/tasks/artifacts/demo" "$P/.claude/tasks/artifacts/sref2"
sed -i 's/retry with backoff/retry with backoff as in s2/' "$P/.claude/tasks/artifacts/sref2/pr.md"
cp -R "$P/.claude/tasks/artifacts/demo" "$P/.claude/tasks/artifacts/vmulti"
sed -i 's/^- s2 — demo TUI/- s1, s2 — demo TUI/' "$P/.claude/tasks/artifacts/vmulti/plan.summary.md"
n=$(env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" vmulti draft 2>/dev/null | grep -c '^  - beyond unit tests: demo TUI$')
[ "$n" = 2 ] && ok pr-body-verif-multi || bad pr-body-verif-multi "a verification line naming two stages shows under both, got $n"
golden pr-body-refused env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" sref2 full
mkdir -p "$P/.claude/tasks/inreview" && cp "$P/.claude/tasks/done/demo.s0.md" "$P/.claude/tasks/inreview/"
golden pr-body-duplicate-card env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" demo draft
P=$(project); gp() { git -C "$P" -c user.name=t -c user.email=t@t -c commit.gpgsign=false "$@"; }
gp init -q -b main; mkdir -p "$P/docs/specs/modbus"
printf '**MB-R-001** — A register has exactly five properties.\n' > "$P/docs/specs/modbus/requirements.md"
gp add docs; gp commit -qm base; gp checkout -q -b feat/mrcpr
printf '**MB-R-294** — `Esc` opens the close-confirmation popup.\n**MB-R-301** — The error box tracks the inputs live.\n**MB-R-293** — The inputs are trimmed.\n**MB-R-292** — `Enter` is refused while an input fails.\n' >> "$P/docs/specs/modbus/requirements.md"
gp commit -qam branch
printf '**MB-R-295** — Polling continues.\n' >> "$P/docs/specs/modbus/requirements.md"
CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=2026-10-16T20:31 sh "$scripts/card-log.sh" mrcpr.s3 spec-delta MB-R-295 added origin=impl -- polls on >/dev/null
CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=2026-10-16T20:30 sh "$scripts/card-log.sh" mrcpr.s3 spec-delta MB-R-294 changed quote=Yes origin=user -- popup on Esc >/dev/null
CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=2026-10-16T20:32 sh "$scripts/card-log.sh" mrcpr.s3 spec-delta MB-R-292 changed "quote=Keep it open." origin=user -- refuse on Enter >/dev/null
golden pr-body-mrcpr-draft-live env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" mrcpr draft
sed -i '/^branch:/d' "$P/.claude/tasks/inreview/mrcpr.md"
golden pr-body-mrcpr-draft-nobranch env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" mrcpr draft

# --- prompt-lines.sh
P=$(project); A=$P/.claude/tasks/artifacts/demo
golden prompt-lines-spec-diff sh "$scripts/prompt-lines.sh" "$A/spec-diff.md"
golden prompt-lines-decisions sh "$scripts/prompt-lines.sh" "$A/decisions.md"
golden prompt-lines-decisions-seen env DECISIONS_SEEN=7 sh "$scripts/prompt-lines.sh" "$A/decisions.md"
golden prompt-lines-architecture sh "$scripts/prompt-lines.sh" "$A/architecture.summary.md"
golden prompt-lines-plan sh "$scripts/prompt-lines.sh" "$A/plan.summary.md"
golden prompt-lines-verdict sh "$scripts/prompt-lines.sh" "$A/review.verdict.md"
golden prompt-lines-titles sh "$scripts/prompt-lines.sh" "$A/issue.md" "$A/split-issue.md" "$A/pr.md" "$A/follow-up-1.md"
golden prompt-lines-question sh "$scripts/prompt-lines.sh" "$A/question.md"
golden prompt-lines-violated sh "$scripts/prompt-lines.sh" "$P/.claude/tasks/artifacts/violated/spec-diff.md"
golden prompt-lines-slices sh "$scripts/prompt-lines.sh" "$P/.claude/tasks/artifacts/lb/slices.md"
cp -R "$P/.claude/tasks/artifacts/lb" "$tmp/lbgap"
sed -i 's/^Entries: R3$/Entries: none/; s/^Entries: R1$/Entries: R1, E1/' "$tmp/lbgap/slices.md"
golden prompt-lines-slices-gaps sh "$scripts/prompt-lines.sh" "$tmp/lbgap/slices.md"
golden prompt-lines-unreadable sh "$scripts/prompt-lines.sh" "$A/nosuch.md"

# --- issue-request.sh, with issue-view.sh stubbed to print a fixture
mkdir -p "$tmp/ir"; cp "$scripts/issue-request.sh" "$tmp/ir/"
printf '#!/bin/sh\ncat "$ISSUE_FIXTURE"\n' > "$tmp/ir/issue-view.sh"
golden issue-request-plain env ISSUE_FIXTURE="$fx/issue/plain.txt" sh "$tmp/ir/issue-request.sh" 7
golden issue-request-assembled env ISSUE_FIXTURE="$fx/issue/assembled.txt" sh "$tmp/ir/issue-request.sh" 8
golden issue-request-spec env ISSUE_FIXTURE="$fx/issue/assembled.txt" sh "$tmp/ir/issue-request.sh" 8 --spec
golden issue-request-epic env ISSUE_FIXTURE="$fx/issue/slice.txt" sh "$tmp/ir/issue-request.sh" 71 --epic
golden issue-request-epic-none env ISSUE_FIXTURE="$fx/issue/plain.txt" sh "$tmp/ir/issue-request.sh" 7 --epic
golden issue-request-slice-spec env ISSUE_FIXTURE="$fx/issue/slice.txt" sh "$tmp/ir/issue-request.sh" 71 --spec
golden issue-request-spec-plain env ISSUE_FIXTURE="$fx/issue/plain.txt" sh "$tmp/ir/issue-request.sh" 7 --spec
golden issue-request-new env ISSUE_FIXTURE="$fx/issue/new.txt" sh "$tmp/ir/issue-request.sh" 9
golden issue-request-new-spec env ISSUE_FIXTURE="$fx/issue/new.txt" sh "$tmp/ir/issue-request.sh" 9 --spec

# --- related-ids.sh
P=$(project); mkdir -p "$P/docs"; ln -s "$fx/specs" "$P/docs/specs"
golden related-ids env CLAUDE_PROJECT_DIR="$P" sh "$scripts/related-ids.sh" polls slow
golden related-ids-none env CLAUDE_PROJECT_DIR="$P" sh "$scripts/related-ids.sh" nosuchword
golden related-ids-literal env CLAUDE_PROJECT_DIR="$P" sh "$scripts/related-ids.sh" "retr.ed"

# --- card-field.sh
P=$(project)
golden card-field env CLAUDE_PROJECT_DIR="$P" sh -c 'for f in lane issue split log:cov log:review pass-sha nosuch; do printf "%s=" "$f"; sh "$0" demo "$f"; echo; done; sh "$0" nosuch lane' "$scripts/card-field.sh"

# --- extract-section.sh, list-sections.sh, extract-id.sh
golden list-sections sh "$scripts/list-sections.sh" "$fx/md/doc.md"
golden extract-section sh "$scripts/extract-section.sh" '## Alpha' '## Beta' "$fx/md/doc.md"
golden extract-section-missing sh "$scripts/extract-section.sh" '## Gamma' "$fx/md/doc.md"
golden extract-section-prefix sh "$scripts/extract-section.sh" --prefix '## Al' '## Beta' "$fx/md/doc.md"
golden extract-section-prefix-missing sh "$scripts/extract-section.sh" --prefix '## Zz' "$fx/md/doc.md"
golden extract-id sh -c 'cd "$1" && sh "$0" specs MB-R-002 MB-E-001' "$scripts/extract-id.sh" "$fx"

# --- show-file.sh
P=$(project); A=$P/.claude/tasks/artifacts/demo
golden show-file env SHOW_FILE_VIEWER=none sh -c 'sh "$0" "$1" "$2" | sed "s#$3#<P>#g"' "$scripts/show-file.sh" "$A/plan.summary.md" "$A/review.verdict.md" "$P"
if [ "$(cat "$A/approval" 2>/dev/null)" = "$(printf '%s\n%s' "$A/plan.summary.md" "$A/review.verdict.md")" ]; then ok show-file-marker; else bad show-file-marker "approval marker lists the shown files"; fi
seq 1 40 | sed 's/^/- line /' >> "$A/plan.summary.md"
SHOW_FILE_VIEWER=none sh "$scripts/show-file.sh" "$A/plan.summary.md" >/dev/null 2>&1
[ $? -eq 3 ] && ok show-file-cap || bad show-file-cap "an over-cap summary exits 3"

# --- gauntlet.sh: argument handling only (the build steps need cargo)
P=$(project)
out=$(cd "$tmp" && CLAUDE_PROJECT_DIR="$P" sh "$scripts/gauntlet.sh" "$tmp/no-worktree" artifacts/demo 2>/dev/null)
[ "$out" = "gauntlet=fail step=cd reason=no-worktree" ] && ok gauntlet-no-worktree || bad gauntlet-no-worktree "$out"
[ -f "$P/.claude/tasks/artifacts/demo/gauntlet.log" ] && [ ! -e "$tmp/artifacts" ] && ok gauntlet-artifacts-shorthand || bad gauntlet-artifacts-shorthand "artifacts/<slug> resolves under the board"
rm -f "$P/.claude/tasks/artifacts/demo/gauntlet.log"
(cd "$tmp" && CLAUDE_PROJECT_DIR="$P" sh "$scripts/gauntlet.sh" "$tmp/no-worktree" .claude/tasks/artifacts/demo >/dev/null 2>&1)
[ -f "$P/.claude/tasks/artifacts/demo/gauntlet.log" ] && [ ! -e "$tmp/.claude" ] && ok gauntlet-tasks-relative || bad gauntlet-tasks-relative "a relative path resolves under the project root"

# gauntlet.sh pass cache: keyed by the worktree's tree, untracked files included
rm -rf "$tmp/g"; git init -q -b main "$tmp/g"; echo a > "$tmp/g/a.txt"
git -C "$tmp/g" add a.txt; git -C "$tmp/g" -c user.name=t -c user.email=t@t commit -qm a
G=$P/.claude/tasks/artifacts/demo; echo "$(git -C "$tmp/g" rev-parse 'HEAD^{tree}') 81.2%" > "$G/gauntlet.pass"
out=$(sh "$scripts/gauntlet.sh" "$tmp/g" "$G" 2>/dev/null)
[ "$out" = "gauntlet=pass cov=81.2% sha=$(git -C "$tmp/g" rev-parse --short HEAD) cached" ] && ok gauntlet-cached || bad gauntlet-cached "$out"
out=$(GAUNTLET_FORCE=1 sh "$scripts/gauntlet.sh" "$tmp/g" "$G" 2>/dev/null)
case $out in "gauntlet=fail step=fmt "*) ok gauntlet-force ;; *) bad gauntlet-force "$out" ;; esac
echo b > "$tmp/g/b.txt"
out=$(sh "$scripts/gauntlet.sh" "$tmp/g" "$G" 2>/dev/null)
case $out in "gauntlet=fail step=fmt "*) ok gauntlet-untracked-misses-cache ;; *) bad gauntlet-untracked-misses-cache "$out" ;; esac
[ "$(git -C "$tmp/g" status --porcelain)" = "?? b.txt" ] && ok gauntlet-index-untouched || bad gauntlet-index-untouched "the worktree's own index stays as it was"

# --- metrics.sh
P=$(project); D=$P/.claude/tasks
cl() { id=$1; shift; env CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=$NOW sh "$scripts/card-log.sh" "$id" "$@"; }
NOW=2026-10-03T08:50 cl demo stop kind=approval
NOW=2026-10-03T09:30 cl demo stop kind=question
NOW=2026-10-03T10:40 cl demo replan from=s2
NOW=2026-10-03T10:50 cl demo stop kind=approval
NOW=2026-10-03T10:05 cl demo.s1 test-red MB-R-161 -- it_backoff_caps
NOW=2026-10-03T11:05 cl demo overturned scope=stage count=2 -- comment restates the code
NOW=2026-10-03T11:06 cl demo overturned scope=branch count=1 -- docs stale
NOW=2026-10-03T11:07 cl demo.s1 read path=ferrowl-modbus/src/client_core.rs -- no signature for next_delay
golden metrics env CLAUDE_PROJECT_DIR="$P" METRICS_NOW=2026-10-04T12:00 sh "$scripts/metrics.sh" demo
[ "$(cat "$D/metrics.log")" = "$(sed -n 1p "$fx/expected/metrics.out")" ] && ok metrics-log || bad metrics-log "the printed line is appended to metrics.log"
golden metrics-no-card env CLAUDE_PROJECT_DIR="$P" sh "$scripts/metrics.sh" nosuch

# --- card-log.sh
P=$(project); D=$P/.claude/tasks
cltail() { id=$1; f=$2; shift 2; env CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=2026-10-05T09:00 sh "$scripts/card-log.sh" "$id" "$@" || return $?; tail -1 "$f"; }
golden card-log-ok cltail demo.s1 "$D/inreview/demo.s1.md" spec-delta MB-R-161 changed origin=user 'quote=keep retrying' -- retry forever
golden card-log-parent cltail demo "$D/inprogress/demo.md" stop kind=approval gate=plan
golden card-log-test-red-noid cltail demo.s1 "$D/inreview/demo.s1.md" test-red -- ut_status_line
sum=$(cksum "$D"/*/demo*.md)
clrej() { env CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=2026-10-05T09:00 sh "$scripts/card-log.sh" "$@"; }
golden card-log-reject-event clrej demo.s1 frobnicate
golden card-log-reject-key clrej demo.s1 green foo=1
golden card-log-reject-scope clrej demo.s1 stop kind=question
golden card-log-reject-value clrej demo.s1 checks=maybe
golden card-log-reject-length clrej demo.s1 stopped -- "$(printf 'x%.0s' $(seq 1 160))"
golden card-log-reject-stop-kind clrej demo stop gate=plan
golden card-log-reject-stop-delta clrej demo stop kind=delta
golden card-log-reject-stop-replan clrej demo stop kind=replan
golden card-log-reject-event-quote clrej demo.s1 'commit=a"b'
golden card-log-no-card clrej nosuch.s9 green
cp "$D/inreview/demo.s1.md" "$D/open/demo.s1.md"
golden card-log-dup-card clrej demo.s1 green
rm "$D/open/demo.s1.md"
[ "$sum" = "$(cksum "$D"/*/demo*.md)" ] && ok card-log-reject-unchanged || bad card-log-reject-unchanged "a rejected call must not touch a card"
golden card-log-minor-moved cltail demo.s1 "$D/inreview/demo.s1.md" 'minor-moved=wording -> s3' -- the template
golden card-log-files-add cltail demo.s1 "$D/inreview/demo.s1.md" 'files+=CONTRIBUTING.md, a.md' user=approved reason=review
golden card-log-note cltail demo.s1 "$D/inreview/demo.s1.md" note -- renamed '"attr-a"' to attr-b
golden card-log-note-parent cltail demo "$D/inprogress/demo.md" note -- plan amended after review
golden card-log-note-empty clrej demo.s1 note
golden card-log-accept-160 cltail demo.s1 "$D/inreview/demo.s1.md" stopped -- "$(printf 'x%.0s' $(seq 1 135))"
for k in 1 2 3; do clrej demo.s2 read path=f$k.rs -- the plan names no signature; done
golden card-log-read-budget clrej demo.s2 read path=f4.rs -- still thin
golden card-log-read-no-text clrej demo.s1 read path=f1.rs

# --- card-log.sh vocabulary covers every event the docs name
root=$(cd "$scripts/../.." && pwd)
names=$({ grep -ohE '[Ll]ogs? (`[^`]+`)( or `[^`]+`)*' "$root/.claude/AGENTS.workflow.md" "$root"/.claude/agents/*.md | grep -oE '`[^`]+`'
          grep -oE 'card-log\.sh <id> [^ ]+' "$root/.claude/agents/spec-implementer.md" | sed 's/.* /`/'; } \
    | sed -E 's/^`//; s/[ `].*//; s/^([a-z][a-z+-]*=?).*/\1/' | grep -E '^[a-z]' | sort -u)
vocab=$(sed -n '/^# vocab-begin/,/^# vocab-end/p' "$scripts/card-log.sh")
missing=""
for n in $names; do echo "$vocab" | awk -v n="$n" '$3 == n { f = 1 } END { exit !f }' || missing="$missing $n"; done
[ -z "$missing" ] && ok card-log-vocab-complete || bad card-log-vocab-complete "missing:$missing"

# --- hook guards
P=$(project); seq 1 100 > "$P/big.txt"; printf '# x\n' > "$P/notes.md"
hook shell-cat-md hook-guard-shell.sh 'cat notes.md' deny
hook shell-cat-md-piped hook-guard-shell.sh 'cat notes.md | head -5' allow
hook shell-cat-large hook-guard-shell.sh 'cat big.txt' deny
hook shell-gh-issue-view hook-guard-shell.sh 'gh issue view 12' deny
hook shell-gh-pr-view hook-guard-shell.sh 'gh pr view 34' deny
hook shell-git-diff-bare hook-guard-shell.sh 'git diff' deny
hook shell-git-diff-ref hook-guard-shell.sh 'git diff main...HEAD' deny
hook shell-git-diff-stat hook-guard-shell.sh 'git diff --stat' allow
hook shell-git-diff-path hook-guard-shell.sh 'git diff main -- src' allow
hook shell-ls hook-guard-shell.sh 'ls -la' allow
hook shell-append-card hook-guard-shell.sh 'echo x >> .claude/tasks/open/demo.s2.md' deny
hook shell-tee-card hook-guard-shell.sh 'grep a b | tee -a /x/.claude/tasks/inprogress/demo.md' deny
hook shell-heredoc-card hook-guard-shell.sh 'cat >> .claude/tasks/done/demo.s0.md <<EOF
x
EOF' deny
hook shell-tee-append-long hook-guard-shell.sh 'echo x | tee --append .claude/tasks/open/a.s1.md' deny
hook shell-tee-dashdash hook-guard-shell.sh 'echo x | tee -a -- .claude/tasks/open/a.s1.md' deny
hook shell-append-other-done hook-guard-shell.sh 'echo x >> /tmp/done/notes.md' allow
hook shell-append-artifact hook-guard-shell.sh 'echo x >> .claude/tasks/artifacts/demo/review.md' allow
hook shell-card-log hook-guard-shell.sh 'sh .claude/scripts/card-log.sh demo green' allow
hook ro-git-checkout hook-guard-readonly.sh 'git checkout src/x.rs' deny
hook ro-git-diff hook-guard-readonly.sh 'git diff main...HEAD -- src' allow
hook ro-git-grep hook-guard-readonly.sh 'git grep -n MB-R-161 main -- docs/specs' allow
hook ro-append-review hook-guard-readonly.sh 'echo x >> .claude/tasks/artifacts/demo/review.md' allow
hook ro-write-verdict hook-guard-readonly.sh 'cat > .claude/tasks/artifacts/demo/review.verdict.md <<EOF
x
EOF' allow
hook ro-write-other hook-guard-readonly.sh 'echo x > src/lib.rs' deny
hook ro-rm hook-guard-readonly.sh 'rm -rf target' deny
hook attr-trailer hook-guard-attribution.sh 'git commit -m "fix: x

Co-Authored-By: Someone <a@b.c>"' deny
hook attr-generated hook-guard-attribution.sh 'gh pr create --title t --body "Generated with a tool"' deny
hook attr-clean hook-guard-attribution.sh 'git commit -m "fix: x"' allow
hook attr-generated-line hook-guard-attribution.sh 'git commit -F - <<EOF
fix: x

🤖 Generated with a tool
EOF' deny
hook attr-trailer-option-quoted hook-guard-attribution.sh 'git commit --trailer "Co-Authored-By: X <a@b.c>" -m x' deny
hook attr-trailer-option hook-guard-attribution.sh 'git commit --trailer Co-authored-by:X -m x' deny
hook attr-trailer-eq hook-guard-attribution.sh 'git commit --trailer=Co-authored-by:X -m x' deny
hook attr-list-footer hook-guard-attribution.sh 'git commit -F - <<EOF
fix: x

- 🤖 Generated with a tool
EOF' deny
hook attr-italic-footer hook-guard-attribution.sh 'git commit -F - <<EOF
fix: x

*Generated with a tool*
EOF' deny
hook attr-marker-after-emoji hook-guard-attribution.sh 'git commit -F - <<EOF
fix: x

🤖 *Generated with a tool*
EOF' deny
hook attr-trailer-eqsep hook-guard-attribution.sh 'git commit --trailer Co-authored-by=A -m x' deny
hook attr-escaped-newline hook-guard-attribution.sh 'gh pr create --title t --body "x\n\nCo-authored-by: A"' deny
hook attr-quote-footer hook-guard-attribution.sh 'git commit -F - <<EOF
fix: x

> Generated with a tool
EOF' deny
hook attr-regenerated hook-guard-attribution.sh 'git commit -m "docs: images regenerated with images/capture.py"' allow
hook attr-mention hook-guard-attribution.sh 'git commit -m "fix: hook no longer matches generated with in prose"' allow

# --- git-state guards and run-cleanup.sh, on a scratch repository with a bare remote
g() { d=$1; shift; git -C "$d" -c user.name=t -c user.email=t@t -c commit.gpgsign=false "$@"; }
repo() {
    rm -rf "$tmp/r" "$tmp/remote.git"
    git init -q --bare "$tmp/remote.git"
    git init -q -b main "$tmp/r"
    mkdir -p "$tmp/r/.claude"; cp -R "$fx/board" "$tmp/r/.claude/tasks"
    printf '.claude/tasks/\n.claude/worktrees/\n' > "$tmp/r/.gitignore"
    g "$tmp/r" add .gitignore >/dev/null; g "$tmp/r" commit -q -m init
    g "$tmp/r" remote add origin "$tmp/remote.git"; g "$tmp/r" push -q origin main 2>/dev/null
    git -C "$tmp/r" worktree add -q "$tmp/r/.claude/worktrees/demo" -b feat/demo main
    W=$tmp/r/.claude/worktrees/demo
    printf 'x\n' > "$W/x.txt"; g "$W" add x.txt; g "$W" commit -q -m stage
    g "$W" push -q -u origin feat/demo 2>/dev/null
}
HOOK_CWD=$tmp/r
repo
hook shell-commit-on-main hook-guard-shell.sh 'git commit -m x' deny
hook shell-push-main hook-guard-shell.sh 'git push origin main' deny
HOOK_CWD=$W
hook shell-commit-on-branch hook-guard-shell.sh 'git commit -m x' allow
unset HOOK_CWD

(unset CLAUDE_PROJECT_DIR; cd "$W" && sh "$scripts/card-log.sh" demo.s1 green)
tail -1 "$tmp/r/.claude/tasks/inreview/demo.s1.md" | grep -q ' green$' && ok card-log-worktree || bad card-log-worktree "a worktree finds the main checkout's board"

cleanup() { CLAUDE_PROJECT_DIR="$tmp/r" sh "$scripts/run-cleanup.sh" demo; }
printf 'y\n' > "$W/y.txt"
out=$(cleanup); [ "$out" = "cleanup=refused reason=uncommitted-changes" ] && [ -d "$W" ] && ok cleanup-refuses-dirty || bad cleanup-refuses-dirty "$out"
g "$W" add y.txt; g "$W" commit -q -m more
out=$(cleanup); [ "$out" = "cleanup=refused reason=unpushed-commits" ] && [ -d "$tmp/r/.claude/tasks/artifacts/demo" ] && ok cleanup-refuses-unpushed || bad cleanup-refuses-unpushed "$out"
g "$W" push -q 2>/dev/null
printf 'target/\n' >> "$W/.gitignore"; g "$W" add .gitignore; g "$W" commit -q -m ignore; g "$W" push -q 2>/dev/null
mkdir -p "$W/target" && printf 'build\n' > "$W/target/out"
out=$(cleanup)
B=$tmp/r/.claude/tasks
if [ "$out" = "cleanup=done archive=$B/archive/demo" ] && [ ! -e "$W" ] && ! git -C "$tmp/r" rev-parse --verify -q feat/demo >/dev/null \
    && git -C "$tmp/remote.git" rev-parse --verify -q feat/demo >/dev/null \
    && [ ! -e "$B/artifacts/demo" ] && [ -z "$(ls "$B"/*/demo.md "$B"/*/demo.s*.md 2>/dev/null)" ] \
    && [ -e "$B/archive/demo/cards/demo.md" ] && [ -e "$B/archive/demo/cards/demo.s0.md" ] && [ -e "$B/archive/demo/artifacts/issue.md" ] \
    && [ -e "$B/artifacts/nospec/issue.md" ] && [ -e "$B/inreview/nospec.md" ]; then ok cleanup-done
else bad cleanup-done "$out"; fi
out=$(cleanup); [ "$out" = "cleanup=done archive=none" ] && ok cleanup-idempotent || bad cleanup-idempotent "$out"
mkdir -p "$B/artifacts/demo"; echo x > "$B/artifacts/demo/issue.md"; printf -- '---\nid: demo\n---\n' > "$B/inprogress/demo.md"
out=$(cleanup)
case $out in "cleanup=done archive=$B/archive/demo-"[0-9]*T[0-9]*Z) [ -e "${out#*archive=}/cards/demo.md" ] && ok cleanup-collision || bad cleanup-collision "$out" ;; *) bad cleanup-collision "$out" ;; esac
leftover() { mkdir -p "$B/artifacts/demo"; printf -- '---\nid: demo\n---\n' > "$B/inprogress/demo.md"; }
leftover; out2=$(CLEANUP_NOW=20260101T000000Z cleanup)
leftover; out3=$(CLEANUP_NOW=20260101T000000Z cleanup)
[ "$out2" = "cleanup=done archive=$B/archive/demo-20260101T000000Z" ] && [ "$out3" = "cleanup=done archive=$B/archive/demo-20260101T000000Z-1" ] \
    && [ -e "$B/archive/demo-20260101T000000Z/cards/demo.md" ] && [ -e "$B/archive/demo-20260101T000000Z-1/cards/demo.md" ] && ok cleanup-same-second || bad cleanup-same-second "$out2 / $out3"
if [ "$(id -u)" != 0 ]; then
    leftover; chmod a-w "$B/archive"
    out=$(CLEANUP_NOW=20260101T000001Z cleanup); chmod u+w "$B/archive"
    [ "$out" = "cleanup=refused reason=archive-failed" ] && [ -e "$B/inprogress/demo.md" ] && ok cleanup-archive-unwritable || bad cleanup-archive-unwritable "$out"
    rm -rf "$B/artifacts/demo" "$B/inprogress/demo.md"
fi

# --- run-rebuild.sh: a lost board rebuilt from a stubbed issue, a stubbed draft PR and the branch
g "$tmp/r" checkout -q main
mkdir -p "$tmp/r/docs/specs/modbus"
printf '## Client\n\n**MB-R-001** — The client connects.\n' > "$tmp/r/docs/specs/modbus/requirements.md"
g "$tmp/r" add docs; g "$tmp/r" commit -q -m spec; g "$tmp/r" push -q origin main 2>/dev/null
git -C "$tmp/r" worktree add -q "$tmp/rbsrc" -b docs/rb main
printf '## Client\n\n**MB-R-001** — The client connects over TCP.\n' > "$tmp/rbsrc/docs/specs/modbus/requirements.md"
g "$tmp/rbsrc" commit -q -am change; SHA0=$(git -C "$tmp/rbsrc" rev-parse --short HEAD)
printf '**MB-R-002** — The client polls.\n' >> "$tmp/rbsrc/docs/specs/modbus/requirements.md"
printf '## Client\n\n| ID | Condition | Behavior |\n|---|---|---|\n| **MB-E-001** | Slow server | poll skipped |\n' > "$tmp/rbsrc/docs/specs/modbus/edge-cases.md"
g "$tmp/rbsrc" add docs; g "$tmp/rbsrc" commit -q -am more; SHA1=$(git -C "$tmp/rbsrc" rev-parse --short HEAD)
g "$tmp/rbsrc" push -q origin docs/rb 2>/dev/null
git -C "$tmp/r" worktree remove --force "$tmp/rbsrc"; git -C "$tmp/r" branch -D docs/rb >/dev/null
mkdir -p "$tmp/rb" "$tmp/bin"
cp "$scripts/run-rebuild.sh" "$scripts/card-log.sh" "$scripts/card-field.sh" "$tmp/rb/"
printf '#!/bin/sh\ncat "$ISSUE_FIXTURE"\n' > "$tmp/rb/issue-view.sh"
printf '#!/bin/sh\nsed "s/SHA0/$SHA0/; s/SHA1/$SHA1/" "$PR_FIXTURE"\n' > "$tmp/rb/pr-view.sh"
printf '#!/bin/sh\necho docs/rb\n' > "$tmp/bin/gh"; chmod +x "$tmp/bin/gh"
RB=$tmp/r/.claude/tasks
rb_clear() { rm -rf "$RB"/artifacts/rb "$RB"/*/rb.md "$RB"/*/rb.s[0-9]*.md; }
rb_run() {
    (cd "$tmp/r" && PATH=$tmp/bin:$PATH CLAUDE_PROJECT_DIR=$tmp/r ISSUE_FIXTURE=${RB_ISSUE:-$fx/issue/rebuild.txt} PR_FIXTURE=${RB_PR:-$fx/pr/${1:-rebuild}.txt} SHA0=$SHA0 SHA1=$SHA1 sh "$tmp/rb/run-rebuild.sh" 21 22)
}
rb_files() {
    for f in inprogress/rb.md done/rb.s0.md done/rb.s1.md open/rb.s2.md artifacts/rb/issue.md artifacts/rb/spec-diff.md artifacts/rb/plan.summary.md artifacts/rb/architecture.summary.md artifacts/rb/architecture.md; do
        echo "==> $f <=="; cat "$RB/$f"
    done | sed -E "s/20[0-9]{2}-[0-9]{2}-[0-9]{2}(T[0-9]{2}:[0-9]{2})?/<today>/; s/$SHA0/SHA0/; s/$SHA1/SHA1/"
}
rb_clear
golden run-rebuild-done rb_run
golden run-rebuild-files rb_files
git -C "$tmp/r" worktree list | grep -q '\.claude/worktrees/rb ' && ok run-rebuild-worktree || bad run-rebuild-worktree "the branch is checked out at .claude/worktrees/rb"
golden run-rebuild-exists rb_run
rb_clear
golden run-rebuild-no-stages rb_run ready

rb_clear
sed 's/SHA0/deadbee/' "$fx/pr/rebuild.txt" > "$tmp/pr-badsha.txt"
RB_PR=$tmp/pr-badsha.txt golden run-rebuild-sha rb_run
[ -z "$(ls "$RB"/*/rb.md "$RB"/*/rb.s[0-9]*.md 2>/dev/null)" ] && [ ! -e "$RB/artifacts/rb" ] && ok run-rebuild-sha-writes-nothing || bad run-rebuild-sha-writes-nothing "a refused rebuild leaves the board alone"
RB_ISSUE=$tmp/none.txt golden run-rebuild-no-issue rb_run
mkdir -p "$RB/artifacts/rb"
golden run-rebuild-exists-artifacts rb_run
rb_clear
rm -rf "$tmp/r/.claude/worktrees/rb"
golden run-rebuild-stale rb_run
[ -d "$tmp/r/.claude/worktrees/rb/docs" ] && ok run-rebuild-stale-worktree || bad run-rebuild-stale-worktree "a registered but missing worktree is re-added"

rb_clear
git -C "$tmp/r" worktree remove --force "$tmp/r/.claude/worktrees/rb"; : > "$tmp/r/.claude/worktrees/rb"
golden run-rebuild-worktree-refused rb_run
[ -z "$(ls "$RB"/*/rb.md "$RB"/*/rb.s[0-9]*.md 2>/dev/null)" ] && [ ! -e "$RB/artifacts/rb" ] && ok run-rebuild-worktree-writes-nothing || bad run-rebuild-worktree-writes-nothing "a refused worktree leaves the board alone"
rm -f "$tmp/r/.claude/worktrees/rb"

# --- id-collisions.sh: the branch and main both define the next free ID
rm -rf "$tmp/ids"; git init -q -b main "$tmp/ids"; mkdir -p "$tmp/ids/docs/specs"
printf '**MB-R-001** — The client connects.\n' > "$tmp/ids/docs/specs/r.md"
g "$tmp/ids" add . ; g "$tmp/ids" commit -q -m base
g "$tmp/ids" checkout -q -b feat/x
printf '**MB-R-002** — The client polls.\n**MB-R-003** — The client retries.\n' >> "$tmp/ids/docs/specs/r.md"; g "$tmp/ids" commit -q -am x
out=$(sh "$scripts/id-collisions.sh" "$tmp/ids" main); [ "$out" = ids=clean ] && ok ids-clean || bad ids-clean "$out"
g "$tmp/ids" checkout -q main
printf '**MB-R-002** — The server answers.\n' >> "$tmp/ids/docs/specs/r.md"; g "$tmp/ids" commit -q -am y
g "$tmp/ids" checkout -q feat/x
out=$(sh "$scripts/id-collisions.sh" "$tmp/ids" main); [ "$out" = "ids=collide MB-R-002" ] && ok ids-collide || bad ids-collide "$out"

# --- decision scripts on a scratch run "ss": a main checkout with the board and the run's worktree
is() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "expected '$2', got '$3'"; fi; }
sscard() { # <n> <state> [<frontmatter line>]
    b="[]"; [ "$1" -gt 0 ] && b="[ss.s$(($1 - 1))]"
    printf -- '---\nid: ss.s%s\nparent: ss\ntitle: Stage %s\nblocked-by: %s\nfiles: [f%s.txt]\n%s---\n' "$1" "$1" "$b" "$1" "${3:+$3
}" > "$SB/$2/ss.s$1.md"
}
ssrun() { # <number of stages besides s0>
    rm -rf "$tmp/s"; git init -q -b main "$tmp/s"
    printf '.claude/\n' > "$tmp/s/.gitignore"; g "$tmp/s" add .gitignore; g "$tmp/s" commit -q -m init
    SB=$tmp/s/.claude/tasks; SW=$tmp/s/.claude/worktrees/ss
    for d in open inprogress inreview done artifacts/ss; do mkdir -p "$SB/$d"; done
    git -C "$tmp/s" worktree add -q "$SW" -b feat/ss main
    printf -- '---\nid: ss\nbranch: feat/ss\nlane: full\ngate1: 2026-10-01\narch: 2026-10-02\ngate2: 2026-10-03\n---\n' > "$SB/inprogress/ss.md"
    sscard 0 done; i=1; while [ "$i" -le "$1" ]; do sscard "$i" open; i=$((i + 1)); done
    printf '# ss — plan summary (rev 1)\n\nDecide: x\n\nVerification beyond unit tests:\n- s3 — demo TUI\n  - see it\n' > "$SB/artifacts/ss/plan.summary.md"
}
run_s() { script=$1; shift; env CLAUDE_PROJECT_DIR="$tmp/s" CARD_LOG_NOW=2026-10-05T09:00 sh "$scripts/$script" "$@"; }

ssrun 5
is stage-stop-s0 auto "$(run_s stage-stop.sh ss s0)"
is stage-stop-auto auto "$(run_s stage-stop.sh ss s1)"
is stage-stop-auto-logged "2026-10-05T09:00 auto=yes" "$(tail -1 "$SB/open/ss.s1.md")"
printf 'Verdict: findings — 0 must fix, 1 needs you\n\nNeeds user\n- keep it?\n  - a) yes\n' > "$SB/artifacts/ss/review.verdict.md"
is stage-stop-needs-user "stop=1 reason=needs-user" "$(run_s stage-stop.sh ss s1)"
is stage-stop-logged "2026-10-05T09:00 stop=1" "$(tail -1 "$SB/open/ss.s1.md")"
printf 'Verdict: clean — ready for approval\n\nNeeds user\n- none\n' > "$SB/artifacts/ss/review.verdict.md"
is stage-stop-verification 'stop=2 reason=verification detail="demo TUI"' "$(run_s stage-stop.sh ss s3)"
is stage-stop-threads 'stop=3 reason=pr-threads detail="2 threads"' "$(run_s stage-stop.sh ss s1 threads=2)"
is stage-stop-last "stop=4 reason=last-stage" "$(run_s stage-stop.sh ss s5)"
sscard 2 open 'stop: yes'
is stage-stop-plan-marked "stop=5 reason=plan-marked" "$(run_s stage-stop.sh ss s2)"
printf 'a\n' > "$SW/a.txt"; printf 'b\nc\n' > "$SW/b.txt"
is stage-stop-diff-size 'stop=6 reason=diff-size detail="3 lines, 2 files"' "$(STAGE_STOP_FILES=1 run_s stage-stop.sh ss s1)"
is stage-stop-diff-under auto "$(STAGE_STOP_FILES=2 run_s stage-stop.sh ss s1)"
rm -f "$SW/a.txt" "$SW/b.txt"
ssrun 5; rm "$SB/artifacts/ss/plan.summary.md"
for n in 1 2 3; do run_s stage-stop.sh ss s$n >/dev/null; done
is stage-stop-checkpoint "stop=7 reason=checkpoint" "$(run_s stage-stop.sh ss s4)"
ssrun 5
run_s card-log.sh ss.s2 stop=5; run_s card-log.sh ss.s3 auto=yes
is stage-stop-checkpoint-reset auto "$(run_s stage-stop.sh ss s4)"
golden stage-stop-no-card run_s stage-stop.sh ss s9

# sha-check.sh
ssrun 2
printf 'x\n' > "$SW/f1.txt"; g "$SW" add f1.txt; g "$SW" commit -q -m s1; S1=$(git -C "$SW" rev-parse --short HEAD)
run_s card-log.sh ss.s1 "commit=$S1"
is sha-check-ok sha=ok "$(run_s sha-check.sh ss s1 "$S1")"
is sha-check-missing "sha=mismatch reason=missing" "$(run_s sha-check.sh ss s1 deadbee)"
g "$tmp/s" commit -q --allow-empty -m on-main; M=$(git -C "$tmp/s" rev-parse --short HEAD)
is sha-check-off-branch "sha=mismatch reason=off-branch" "$(run_s sha-check.sh ss s1 "$M")"
printf 'y\n' >> "$SW/f1.txt"; g "$SW" commit -q -am s1b; S1B=$(git -C "$SW" rev-parse --short HEAD)
is sha-check-card-commit "sha=mismatch reason=card-commit" "$(run_s sha-check.sh ss s1 "$S1B")"
run_s sha-check.sh ss s1 "$S1B" >/dev/null; is sha-check-exit 1 "$?"

# reconcile.sh, on the same run
run_s card-log.sh ss.s0 "commit=$S1"
is reconcile-clean "reconcile=clean next=s1" "$(run_s reconcile.sh ss)"
run_s card-log.sh ss.s2 commit=deadbee
run_s card-log.sh ss gauntlet=pass "sha=$M"
out=$(run_s reconcile.sh ss)
is reconcile-differ "differ row=commit s2 deadbee does not exist
differ row=gauntlet $M not on feat/ss
reconcile=differ n=2 next=s1" "$out"
git -C "$tmp/s" worktree remove --force "$SW"
is reconcile-worktree "differ row=worktree $SW missing" "$(run_s reconcile.sh ss | head -1)"

# card-move.sh
ssrun 2
mv "$SB/done/ss.s0.md" "$SB/open/ss.s0.md"
is card-move-blocked-by "card-move=refused from=open to=inprogress reason=blocked-by" "$(run_s card-move.sh ss.s1 inprogress)"
run_s card-move.sh ss.s0 inprogress; run_s card-move.sh ss.s0 inreview
is card-move-logged "2026-10-05T09:00 moved=inreview" "$(tail -1 "$SB/inreview/ss.s0.md")"
is card-move-no-commit "card-move=refused from=inreview to=done reason=commit" "$(run_s card-move.sh ss.s0 done)"
run_s card-log.sh ss.s0 commit=abc1234
is card-move-unverified "card-move=refused from=inreview to=done reason=unverified" "$(run_s card-move.sh ss.s0 done)"
run_s card-log.sh ss gauntlet=pass sha=abc1234
run_s card-move.sh ss.s0 done; [ -f "$SB/done/ss.s0.md" ] && ok card-move-done || bad card-move-done "verified commit moves to done"
run_s card-move.sh ss.s0 done; is card-move-same 0 "$?"
run_s card-move.sh ss.s1 inprogress; [ -f "$SB/inprogress/ss.s1.md" ] && ok card-move-runnable || bad card-move-runnable "blocked-by done → inprogress"
is card-move-skip "card-move=refused from=open to=done reason=transition" "$(run_s card-move.sh ss.s2 done)"
is card-move-parent-done "card-move=refused from=inprogress to=done reason=transition" "$(run_s card-move.sh ss done)"
mv "$SB/inprogress/ss.md" "$SB/open/ss.md"; sed -i '/^gate2:/d' "$SB/open/ss.md"
is card-move-parent-gate2 "card-move=refused from=open to=inprogress reason=gate2" "$(run_s card-move.sh ss inprogress)"
golden card-move-no-card run_s card-move.sh nosuch.s1 done
P=$(project)
hook shell-mv-card hook-guard-shell.sh 'mv .claude/tasks/open/demo.s2.md .claude/tasks/done/' deny
hook shell-mv-card-into hook-guard-shell.sh 'mv /tmp/x.md .claude/tasks/done/demo.s9.md' deny
hook shell-mv-other hook-guard-shell.sh 'mv notes.md docs/notes.md' allow
hook shell-card-move hook-guard-shell.sh 'sh .claude/scripts/card-move.sh demo.s2 done' allow

# --- replayed runs: the decision scripts in workflow order, fake agents (replay.sh)
for sc in full fast-replan light resume; do golden "replay-$sc" sh "$here/replay.sh" "$sc"; done

# --- prompt-render.sh, on the fixture board
P=$(project); D=$P/.claude/tasks
pr_() { env CLAUDE_PROJECT_DIR="$P" CARD_LOG_NOW=2026-10-05T09:00 SHOW_FILE_VIEWER=none sh "$scripts/prompt-render.sh" "$@"; }
golden prompt-render-gate1 pr_ gate1 demo
golden prompt-render-gate1-epic pr_ gate1 demo size=epic "reason=two areas, 14 entries"
golden prompt-render-arch pr_ arch demo
golden prompt-render-gate2 pr_ gate2 demo issue=new
golden prompt-render-stage pr_ stage demo stage=s1 reason=verification "detail=demo TUI"
golden prompt-render-replan pr_ replan demo from=s2
golden prompt-render-gate3 pr_ gate3 demo
golden prompt-render-gate4 pr_ gate4 demo
golden prompt-render-question pr_ question demo where=gate1 agent=spec-author
golden prompt-render-blocked pr_ blocked demo stage=s2 "reason=the parse_frame reference names no signature"
golden prompt-render-delta pr_ delta demo stage=s1
golden prompt-render-auto pr_ auto demo stage=s1
golden prompt-render-gate2-notice pr_ gate2-notice demo issue=new
golden prompt-render-epic pr_ epic lb
golden prompt-render-stops sh -c 'grep " stop " "$0" | cut -d" " -f2-' "$D/inprogress/demo.md"
P=$(project); D=$P/.claude/tasks
pr_ gate2 demo >/dev/null
golden prompt-render-approved pr_ approved demo
golden prompt-render-since-rev pr_ arch demo
sed -i '/^gate1:/d' "$D/inprogress/demo.md"; sed -i 's/^lane: full/lane: fast/' "$D/inprogress/demo.md"
golden prompt-render-arch-fast pr_ arch demo
seq 1 40 | sed 's/^/- line /' >> "$D/artifacts/demo/plan.summary.md"
pr_ gate2 demo >/dev/null 2>&1; is prompt-render-cap 3 "$?"

echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
