#!/bin/sh
# List a markdown file's headings, one per line, exactly as extract-section.sh
# expects them verbatim as its first argument; a `#` line inside a fenced code
# block is not a heading. Lets an agent discover what's
# extractable without grepping or reading the whole file first.
# Generic: works on any markdown file, not just docs/specs/.
#
# Usage: sh .claude/scripts/list-sections.sh path/to/file.md [path/to/file2.md ...]
set -eu

if [ "$#" -lt 1 ]; then
  echo "Usage: list-sections.sh file.md [file2.md ...]" >&2
  exit 1
fi

for file in "$@"; do
  if [ "$#" -gt 1 ]; then
    echo "== $file =="
  fi
  awk '/^```/ { fence = !fence } !fence && /^#+[ \t]/ { print }' "$file"
done
