#!/bin/sh
# Find one or more spec requirements by ID and print each as `file:line:text`
# — exact spot to edit, no guessing which file an ID lives in. Batch every ID
# needed into one call instead of one invocation per ID.
#
# Usage: sh .claude/scripts/extract-id.sh [spec-dir] <ID> [<ID> ...]
#   spec-dir defaults to docs/specs (autodetected relative to cwd)
set -eu

if [ "$#" -lt 1 ]; then
  echo "Usage: extract-id.sh [spec-dir] <ID> [<ID> ...]" >&2
  exit 1
fi

dir="docs/specs"
if [ -d "$1" ]; then
  dir="$1"
  shift
fi

if [ "$#" -lt 1 ]; then
  echo "Usage: extract-id.sh [spec-dir] <ID> [<ID> ...]" >&2
  exit 1
fi

if [ ! -d "$dir" ]; then
  echo "Spec directory not found: $dir" >&2
  exit 1
fi

missing=""
for id in "$@"; do
  match=$(find "$dir" -name '*.md' -exec grep -n -- "\*\*${id}\*\*" {} +) || match=""
  if [ -z "$match" ]; then
    missing="$missing $id"
  else
    echo "$match"
  fi
done

if [ -n "$missing" ]; then
  echo "No requirement found for ID(s):$missing (searched $dir)" >&2
  exit 1
fi
