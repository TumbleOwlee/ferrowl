#!/usr/bin/env sh
# Run the full build/test/lint/coverage gauntlet in a worktree and print ONE
# status line; everything else goes to <artifacts>/gauntlet.log. The
# orchestrator reads the line, never the log.
# Usage: gauntlet.sh <worktree> <artifacts-dir>
# Prints: gauntlet=pass cov=<lines%> sha=<short>
#     or: gauntlet=fail step=<fmt|clippy|check|test|cov> sha=<short> log=<path>
set -u
wt="${1:?worktree}"; art="${2:?artifacts dir}"
mkdir -p "$art"
log="$art/gauntlet.log"
: > "$log"
cd "$wt" || { echo "gauntlet=fail step=cd reason=no-worktree"; exit 1; }
sha=$(git rev-parse --short HEAD)
run() {
  step="$1"; shift
  printf '### %s: %s\n' "$step" "$*" >> "$log"
  if ! "$@" >> "$log" 2>&1; then
    echo "gauntlet=fail step=$step sha=$sha log=$log"
    exit 1
  fi
}
run fmt    cargo fmt --check
run clippy cargo clippy --workspace -- -D warnings
run check  cargo check --workspace
run test   cargo test --workspace
run cov    cargo llvm-cov --workspace --fail-under-lines 80
cov=$(grep -E '^TOTAL' "$log" | tail -1 | grep -oE '[0-9.]+%' | sed -n 3p)
echo "gauntlet=pass cov=${cov:-?} sha=$sha"
