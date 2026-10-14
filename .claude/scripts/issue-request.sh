#!/bin/sh
# Print a reused issue's original request, for artifacts/<slug>/request.md.
#
#   sh .claude/scripts/issue-request.sh <number>
#
# The issue body as filed, blank lines kept; when that body was assembled by
# this workflow (it carries `## Spec (rev`), only the text inside its
# `<details>` block, so the request is never nested twice.
[ $# -eq 1 ] || { echo "usage: $0 <number>" >&2; exit 2; }
body=$(bash "$(dirname -- "$0")/issue-view.sh" "$1" | awk '/^by / { on = 1; next } /^--- [0-9]+ comment/ { exit } on' | sed -e :a -e '/^\n*$/{ $d; N; ba' -e '}' | sed '1{/^$/d;}')
case $body in
    *"## Spec (rev"*) printf '%s\n' "$body" | awk '/^<\/details>/ { exit } on { print } /<summary>/ { on = 1 }' | sed '1{/^$/d;}' ;;
    *) printf '%s\n' "$body" ;;
esac
