#!/usr/bin/env bash
# Rank files by rough token cost of a full Read (~chars/4), highest first.
# Usage: token-rank.sh <file>...
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: token-rank.sh <file>..." >&2
  exit 1
fi

for f in "$@"; do
  [ -f "$f" ] || { echo "skip (not a file): $f" >&2; continue; }
  chars=$(wc -c <"$f")
  printf '%d\t%d\t%s\n' "$((chars / 4))" "$chars" "$f"
done | sort -t$'\t' -k1,1rn | awk -F'\t' '{printf "~%d tokens\t%d chars\t%s\n", $1, $2, $3}'
