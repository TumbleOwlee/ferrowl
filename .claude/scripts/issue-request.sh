#!/bin/sh
# Print a reused issue's original request, for artifacts/<slug>/request.md.
#
#   sh .claude/scripts/issue-request.sh <number>          > request.md
#   sh .claude/scripts/issue-request.sh <number> --spec   > issue-spec.md
#   sh .claude/scripts/issue-request.sh <number> --epic        # the epic's number, for a slice issue
#   sh .claude/scripts/issue-request.sh <number> --slice-line  # a slice issue's whole `**Part of epic**` line
#   sh .claude/scripts/issue-request.sh <number> --comments    # the issue's comments, for epic.md
#
# The issue body as filed, blank lines kept; when that body was assembled by
# this workflow (it carries `## Scenarios` or `## Spec (rev`, or starts
# `**Status** `), only the text inside its
# `<details>` block, so the request is never nested twice.
# --spec: an assembled body's `## Spec (rev N)`, `## Architecture`,
# `## Requirements` and `## Scenarios` sections — the brief a /spec-request
# ticket carries — for spec-author to
# start from; prints nothing for a body this workflow did not assemble.
# --epic: the number in a slice issue's `**Part of epic** #<e>` status line;
# nothing for any other issue.
case ${2:-} in ''|--spec|--epic|--slice-line|--comments) ;; *) echo "usage: $0 <number> [--spec | --epic | --slice-line | --comments]" >&2; exit 2 ;; esac
[ $# -le 2 ] || { echo "usage: $0 <number> [--spec | --epic | --slice-line | --comments]" >&2; exit 2; }
if [ "${2:-}" = --comments ]; then
    bash "$(dirname -- "$0")/issue-view.sh" "$1" | awk '/^--- [0-9]+ comment/ { on = 1; next } on' | sed '1{/^$/d;}'
    exit 0
fi
body=$(bash "$(dirname -- "$0")/issue-view.sh" "$1" | awk '/^by / { on = 1; next } /^--- [0-9]+ comment/ { exit } on' | sed -e :a -e '/^\n*$/{ $d; N; ba' -e '}' | sed '1{/^$/d;}')
if [ "${2:-}" = --slice-line ]; then
    printf '%s\n' "$body" | grep -m1 '^\*\*Part of epic\*\* #'
    exit 0
fi
if [ "${2:-}" = --epic ]; then
    printf '%s\n' "$body" | sed -n 's/^\*\*Part of epic\*\* #\([0-9][0-9]*\).*/\1/p' | head -1
    exit 0
fi
if [ $# -eq 2 ]; then
    case $body in
        *"## Spec (rev"*|*"## Scenarios"*|"**Status** "*) printf '%s\n' "$body" | awk '/^## (Spec \(rev|Architecture$|Requirements$|Scenarios$)/ { on = 1; print; next } /^## / || /^<details>/ { on = 0 } on' ;;
    esac
    exit 0
fi
case $body in
    *"## Spec (rev"*|*"## Scenarios"*|"**Status** "*) printf '%s\n' "$body" | awk '/^<\/details>/ { exit } on { print } /<summary>/ { on = 1 }' | sed '1{/^$/d;}' ;;
    *) printf '%s\n' "$body" ;;
esac
