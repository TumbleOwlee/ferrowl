#!/usr/bin/env sh
# Run the full build/test/lint/coverage gauntlet in a worktree and print ONE
# status line; everything else goes to <artifacts>/gauntlet.log. The
# orchestrator reads the line, never the log.
# Usage: gauntlet.sh <worktree> <artifacts-dir>
# An <artifacts-dir> written as the workflow's shorthand `artifacts/<slug>`
# resolves under the main checkout's .claude/tasks/, never the caller's cwd.
# Prints: gauntlet=pass cov=<lines%> sha=<short>
#     or: gauntlet=fail step=<fmt|clippy|check|test|cov> sha=<short> log=<path>
#     or: gauntlet=fail step=<fmt|clippy|check|test|cov> sha=<short> reason=timeout log=<path>
set -u
wt="${1:?worktree}"; art="${2:?artifacts dir}"
case $art in artifacts/*) art="${CLAUDE_PROJECT_DIR:-.}/.claude/tasks/$art" ;; esac
mkdir -p "$art"
log="$art/gauntlet.log"
: > "$log"
cd "$wt" || { echo "gauntlet=fail step=cd reason=no-worktree"; exit 1; }
sha=$(git rev-parse --short HEAD)
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
run check  900  cargo check --workspace
run test   1800 cargo test --workspace
run cov    1800 cargo llvm-cov --workspace --fail-under-lines 80
cov=$(grep -E '^TOTAL' "$log" | tail -1 | grep -oE '[0-9.]+%' | sed -n 3p)
echo "gauntlet=pass cov=${cov:-?} sha=$sha"
