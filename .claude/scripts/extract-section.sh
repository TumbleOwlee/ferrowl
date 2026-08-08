#!/bin/sh
# Print one or more markdown sections: each is the heading line through the
# next heading of equal or shallower depth (or EOF). Batch every heading
# needed from one file into a single call. Don't know the exact heading
# text? Run list-sections.sh on the file first instead of grepping for it.
# Generic: works on any markdown file, not just docs/specs/ — use it instead
# of `cat`/Read-whole-file for any *.md wherever only some sections matter.
#
# Usage: sh .claude/scripts/extract-section.sh '<heading>' ['<heading>' ...] <file>
set -eu

if [ "$#" -lt 2 ]; then
  echo "Usage: extract-section.sh '<heading>' ['<heading>' ...] <file>" >&2
  exit 1
fi

for file; do :; done
n_headings=$(( $# - 1 ))

missing=""
i=0
for want; do
  i=$((i + 1))
  [ "$i" -le "$n_headings" ] || break
  found=$(awk -v want="$want" '
    /^#+[ \t]/ {
      n = match($0, /^#+/)
      lvl = RLENGTH
      if (printing && lvl <= start_lvl) { printing = 0 }
      if ($0 == want) { printing = 1; start_lvl = lvl; print; next }
    }
    printing { print }
  ' "$file")
  if [ -z "$found" ]; then
    missing="$missing
$want"
  else
    printf '%s\n' "$found"
  fi
done

if [ -n "$missing" ]; then
  echo "no section matched:$missing" >&2
  exit 1
fi
