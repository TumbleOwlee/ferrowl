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
# where the exact output is not the contract. Scripts that only wrap gh
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

# --- pr-body.sh
golden pr-body-draft env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" demo draft
golden pr-body-full env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" demo full
mkdir -p "$P/.claude/tasks/inreview" && cp "$P/.claude/tasks/done/demo.s0.md" "$P/.claude/tasks/inreview/"
golden pr-body-duplicate-card env CLAUDE_PROJECT_DIR="$P" sh "$scripts/pr-body.sh" demo draft

# --- prompt-lines.sh
P=$(project); A=$P/.claude/tasks/artifacts/demo
golden prompt-lines-spec-diff sh "$scripts/prompt-lines.sh" "$A/spec-diff.md"
golden prompt-lines-since-rev env PROMPT_SINCE_REV=2 sh "$scripts/prompt-lines.sh" "$A/spec-diff.md"
golden prompt-lines-architecture sh "$scripts/prompt-lines.sh" "$A/architecture.summary.md"
golden prompt-lines-plan sh "$scripts/prompt-lines.sh" "$A/plan.summary.md"
golden prompt-lines-verdict sh "$scripts/prompt-lines.sh" "$A/review.verdict.md"
golden prompt-lines-titles sh "$scripts/prompt-lines.sh" "$A/issue.md" "$A/split-issue.md" "$A/pr.md"
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

# --- card-field.sh
P=$(project)
golden card-field env CLAUDE_PROJECT_DIR="$P" sh -c 'for f in lane issue split log:cov log:review pass-sha nosuch; do printf "%s=" "$f"; sh "$0" demo "$f"; echo; done; sh "$0" nosuch lane' "$scripts/card-field.sh"

# --- extract-section.sh, list-sections.sh, extract-id.sh
golden list-sections sh "$scripts/list-sections.sh" "$fx/md/doc.md"
golden extract-section sh "$scripts/extract-section.sh" '## Alpha' '## Beta' "$fx/md/doc.md"
golden extract-section-missing sh "$scripts/extract-section.sh" '## Gamma' "$fx/md/doc.md"
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
if [ "$out" = cleanup=done ] && [ ! -e "$W" ] && ! git -C "$tmp/r" rev-parse --verify -q feat/demo >/dev/null \
    && git -C "$tmp/remote.git" rev-parse --verify -q feat/demo >/dev/null \
    && [ ! -e "$B/artifacts/demo" ] && [ -z "$(ls "$B"/*/demo.md "$B"/*/demo.s*.md 2>/dev/null)" ] \
    && [ -e "$B/artifacts/nospec/issue.md" ] && [ -e "$B/inreview/nospec.md" ]; then ok cleanup-done
else bad cleanup-done "$out"; fi
out=$(cleanup); [ "$out" = cleanup=done ] && ok cleanup-idempotent || bad cleanup-idempotent "$out"

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

echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
