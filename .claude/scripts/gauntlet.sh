#!/usr/bin/env sh
# Run the full build/test/lint/coverage gauntlet in a worktree and print ONE
# status line; everything else goes to <artifacts>/gauntlet.log. The
# orchestrator reads the line, never the log.
# Usage: gauntlet.sh <worktree> <artifacts-dir>
# An <artifacts-dir> written as the workflow's shorthand `artifacts/<slug>`
# resolves under the main checkout's .claude/tasks/, never the caller's cwd.
# A pass is cached in <artifacts>/gauntlet.pass under the worktree's tree
# (tracked and untracked files, as `git add -A` would stage them): the same
# tree again prints the cached pass with `cached` and runs nothing, so a
# commit or push of an already verified tree costs nothing. GAUNTLET_FORCE=1
# ignores the cache. No `cargo check` step: clippy type-checks the same crates.
# Prints: gauntlet=pass cov=<lines%> sha=<short> [cached]
#     or: gauntlet=fail step=<fmt|clippy|test|cov> sha=<short> log=<path>
#     or: gauntlet=fail step=<fmt|clippy|test|cov> sha=<short> reason=timeout log=<path>
set -u
wt="${1:?worktree}"; art="${2:?artifacts dir}"
case $art in artifacts/*) art="${CLAUDE_PROJECT_DIR:-.}/.claude/tasks/$art" ;; esac
mkdir -p "$art"
log="$art/gauntlet.log"
cache="$art/gauntlet.pass"
cd "$wt" || { : > "$log"; echo "gauntlet=fail step=cd reason=no-worktree"; exit 1; }
sha=$(git rev-parse --short HEAD)
# A copy of the worktree's index keeps `git add -A` fast and the real index untouched.
tree=$(idx=$(mktemp) && { cp "$(git rev-parse --git-path index)" "$idx" 2>/dev/null || rm -f "$idx"; } &&
  GIT_INDEX_FILE=$idx git add -A . >/dev/null 2>&1 && GIT_INDEX_FILE=$idx git write-tree; rm -f "$idx")
if [ -n "$tree" ] && [ "${GAUNTLET_FORCE:-}" != 1 ] && [ -f "$cache" ]; then
  read -r ctree ccov < "$cache"
  [ "$ctree" = "$tree" ] && { echo "gauntlet=pass cov=$ccov sha=$sha cached"; exit 0; }
fi
: > "$log"
run() {
  step="$1"; limit="$2"; shift 2
  printf '### %s: %s\n' "$step" "$*" >> "$log"
  if ! timeout "$limit" "$@" >> "$log" 2>&1; then
    rc=$?
    if [ "$rc" -eq 124 ]; then
      echo "gauntlet=fail step=$step sha=$sha reason=timeout log=$log"
    else
      echo "gauntlet=fail step=$step sha=$sha log=$log"
    fi
    exit 1
  fi
}
run fmt    900  cargo fmt --check
run clippy 900  cargo clippy --workspace -- -D warnings
run test   1800 cargo test --workspace
run cov    1800 cargo llvm-cov --workspace --fail-under-lines 80
cov=$(grep -E '^TOTAL' "$log" | tail -1 | grep -oE '[0-9.]+%' | sed -n 3p)
[ -n "$tree" ] && echo "$tree ${cov:-?}" > "$cache"
echo "gauntlet=pass cov=${cov:-?} sha=$sha"
