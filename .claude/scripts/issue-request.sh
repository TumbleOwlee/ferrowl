#!/bin/sh
# Print a reused issue's original request, for artifacts/<slug>/request.md.
#
#   sh .claude/scripts/issue-request.sh <number>          > request.md
#   sh .claude/scripts/issue-request.sh <number> --spec   > issue-spec.md
#
# The issue body as filed, blank lines kept; when that body was assembled by
# this workflow (it carries `## Spec (rev`), only the text inside its
# `<details>` block, so the request is never nested twice.
# --spec: an assembled body's `## Spec (rev N)` and `## Architecture`
# sections — the brief a /spec-request ticket carries — for spec-author to
# start from; prints nothing for a body this workflow did not assemble.
[ $# -eq 1 ] || { [ $# -eq 2 ] && [ "$2" = --spec ]; } || { echo "usage: $0 <number> [--spec]" >&2; exit 2; }
body=$(bash "$(dirname -- "$0")/issue-view.sh" "$1" | awk '/^by / { on = 1; next } /^--- [0-9]+ comment/ { exit } on' | sed -e :a -e '/^\n*$/{ $d; N; ba' -e '}' | sed '1{/^$/d;}')
if [ $# -eq 2 ]; then
    case $body in
        *"## Spec (rev"*) printf '%s\n' "$body" | awk '/^## (Spec \(rev|Architecture$)/ { on = 1 } /^## (Decisions|Goal)$/ || /^<details>/ { on = 0 } on' ;;
    esac
    exit 0
fi
case $body in
    *"## Spec (rev"*) printf '%s\n' "$body" | awk '/^<\/details>/ { exit } on { print } /<summary>/ { on = 1 }' | sed '1{/^$/d;}' ;;
    *) printf '%s\n' "$body" ;;
esac
