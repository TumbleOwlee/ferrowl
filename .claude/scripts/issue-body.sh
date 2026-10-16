#!/bin/sh
# Assemble the tracking issue's body from a run's artifacts: what the user
# gets, never how it is built. Written once, at filing; afterwards
# only its status line (line 1) is rewritten, a filed split included.
#
#   sh .claude/scripts/issue-body.sh <slug> > <out>
#
# Line 1 is `# <title>` (the orchestrator strips it for --title), then:
#   status line     `**Status** planned | in progress | done[ · **Lane** <lane>][ · **Split off** #n, #m]`
#                   from the parent card: dir open → planned, inprogress or
#                   inreview → in progress, a `ci=0` log token → done; lane
#                   and split from its frontmatter, Lane omitted without one
#   issue.md        ## Goal, ## Out of scope (omitted when absent)
#   spec-diff.md    ## Requirements and ## Scenarios through spec-render.sh,
#                   local R<n>/E<n>/S<n> IDs; no table, kind, architecture or
#                   decisions (a non-behavior run has no spec-diff.md)
#   request.md      the reused issue's original body, collapsed (if present)
# An epic run (parent card `kind: epic`) adds ## Architecture from
# architecture.summary.md plus the skeleton's `### Interfaces` from
# architecture.md, and a ## Slices list from slices.md, each slice linked to
# its issue once the card's `slices:` has it.
#
#   sh .claude/scripts/issue-body.sh <slug> --slice <n>
#
# Slice n's own issue: title `<epic title>: <slice title>`, `**Status**
# planned`, the `**Part of epic** #<e>` line (issue-request.sh --epic reads
# it), the slice's goal, and only the requirements and scenarios slices.md
# assigns it. A slice run's own body (card `kind: slice`) keeps the
# `**Part of epic**` line from slice-line.md under its status line.
#
#   sh .claude/scripts/issue-body.sh <slug> --status <live-body-file>
#
# The filed body with line 1 (its first non-blank line) replaced by a fresh
# status line when that line starts `**Status** ` or `**Spec** `; any other
# first line (an old slice body) gets the status line above it.
#
# Exit 3 with `body=refused stage-ref=<token>` when the visible text names a
# stage (`s3`) or an agent: reword issue.md. Code spans holding `.` or `/`,
# fenced blocks and the original request are not scanned.
# `## Scenarios` and the `<details>` block are parsed by issue-request.sh and
# the Resume table — keep them when changing the shape.
usage() { echo "usage: $0 <slug> [--slice <n> | --status <body-file>]" >&2; exit 2; }
slice=; sfile=
case $# in
    1) ;;
    3) case $2 in --slice) slice=$3 ;; --status) sfile=$3 ;; *) usage ;; esac ;;
    *) usage ;;
esac
slug=$1
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
render() { sh "$root/.claude/scripts/spec-render.sh" "$a/spec-diff.md" "$@"; }
nth() { printf '%s' "$1" | tr -d '[] ' | tr ',' '\n' | sed -n "${2}p"; }
# slice n of slices.md as `key<TAB>value` lines: title, goal, depends, scenarios, entries, adds, deferred
slice_field() {
    awk -v n="$1" -v k="$2" '
        /^## Slice [0-9]+: / { cur = $3; sub(/:$/, "", cur); if (cur == n && k == "title") { t = $0; sub(/^## Slice [0-9]+: /, "", t); print t } next }
        cur == n && index($0, k ": ") == 1 { print substr($0, length(k) + 3) }
    ' "$a/slices.md"
}
# an issue.md section's lines without blanks, under its own heading and with a trailing blank line
section() {
    body=$(sed -n "/^## $1\$/,/^## /{ /^## /d; p; }" "$a/issue.md" | sed '/^$/d')
    [ -n "$body" ] && printf '## %s\n%s\n\n' "$1" "$body"
}
status_line() {
    dir=$(basename "$(dirname "$(field path)")")
    case $dir in inprogress|inreview) s="in progress" ;; *) s=planned ;; esac
    [ "$(field log:ci)" = 0 ] && s=done
    line="**Status** $s"
    lane=$(field lane); split=$(field split)
    [ -n "$lane" ] && line="$line · **Lane** $lane"
    [ -n "$split" ] && line="$line · **Split off** $(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
    printf '%s\n' "$line"
}

if [ -n "$sfile" ]; then
    [ -f "$sfile" ] || { echo "issue-body: missing $sfile" >&2; exit 1; }
    live=$(sed '/./,$!d' "$sfile")
    case $live in
        "**Status** "*|"**Spec** "*) status_line; printf '%s\n' "$live" | tail -n +2 ;;
        *) status_line; printf '%s\n' "$live" ;;
    esac
    exit 0
fi

[ -f "$a/issue.md" ] || { echo "issue-body: missing $a/issue.md" >&2; exit 1; }
out=$(mktemp); trap 'rm -f "$out"' EXIT INT TERM
{
if [ -n "$slice" ]; then
    [ -f "$a/slices.md" ] || { echo "issue-body: missing $a/slices.md" >&2; exit 1; }
    title=$(slice_field "$slice" title)
    [ -n "$title" ] || { echo "issue-body: no slice $slice in $a/slices.md" >&2; exit 1; }
    epic=$(field issue); total=$(grep -c '^## Slice [0-9]*: ' "$a/slices.md")
    printf '%s: %s\n\n' "$(head -1 "$a/issue.md")" "$title"
    deps=; for d in $(slice_field "$slice" "Depends on" | tr ',' ' '); do
        case $d in none) ;; *) num=$(nth "$(field slices)" "$d"); deps="$deps${deps:+, }${num:+#}${num:-slice $d}" ;; esac
    done
    printf '**Status** planned\n**Part of epic** #%s · **Slice** %s of %s · **Depends on** %s\n\n' "${epic:-?}" "$slice" "$total" "${deps:-none}"
    printf '## Goal\n%s\n\n' "$(slice_field "$slice" Goal)"
    only=$(printf '%s %s' "$(slice_field "$slice" Scenarios)" "$(slice_field "$slice" Entries)" | tr ',' ' ')
    if [ -f "$a/spec-diff.md" ]; then
        render requirements --only "$only"
        render scenarios --only "$only"
    fi
else
    printf '%s\n\n' "$(head -1 "$a/issue.md")"
    status_line
    [ "$(field kind)" = slice ] && [ -s "$a/slice-line.md" ] && head -1 "$a/slice-line.md"
    echo
    section Goal
    if [ -f "$a/spec-diff.md" ]; then
        render requirements
        render scenarios
    fi
    section "Out of scope"

    if [ "$(field kind)" = epic ] && [ -f "$a/architecture.summary.md" ]; then
        echo "## Architecture"
        sed -n 's/^Decide: //p' "$a/architecture.summary.md"
        echo
        sed -n '/^|/p' "$a/architecture.summary.md"
        echo
        surface=$(awk '/^Public surface:/ { on = 1; next } on && /^- / { if ($0 == "- none") next; print; next } on { on = 0 }' "$a/architecture.summary.md")
        [ -n "$surface" ] && printf 'Public surface:\n%s\n\n' "$surface"
        if [ -f "$a/architecture.md" ]; then
            printf '### Interfaces\n'
            awk '/^```/ { fence = !fence } !fence && /^## / { on = ($0 == "## Interfaces"); next } on' "$a/architecture.md" | sed 's/^### /#### /'
        fi
    fi

    if [ "$(field kind)" = epic ] && [ -f "$a/slices.md" ]; then
        echo "## Slices"
        for n in $(sed -n 's/^## Slice \([0-9]*\): .*/\1/p' "$a/slices.md"); do
            num=$(nth "$(field slices)" "$n")
            if [ -n "$num" ]; then link="#$num"; else link="not filed yet"; fi
            printf -- '- [ ] **Slice %s: %s** — %s\n' "$n" "$(slice_field "$n" title)" "$link"
            printf -- '  - %s\n' "$(slice_field "$n" Goal)"
            printf -- '  - depends on: %s\n' "$(slice_field "$n" "Depends on")"
        done
        echo
    fi

    if [ -s "$a/request.md" ]; then
        echo "<details><summary>Original request</summary>"
        echo
        cat "$a/request.md"
        echo "</details>"
    fi
fi
} | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}' > "$out"
[ -s "$out" ] || exit 1

tok=$(awk '
    /^```/ { fence = !fence; next }
    fence { next }
    /^<details><summary>Original request<\/summary>/ { skip = 1 }
    skip { if ($0 ~ /^<\/details>/) skip = 0; next }
    {
        line = $0; gsub(/<!--[^>]*-->/, "", line)
        n = split(line, seg, "`"); line = ""
        for (i = 1; i <= n; i++) if (i % 2 || seg[i] !~ /[.\/]/) line = line seg[i] " "
        if (match(line, /(^|[^A-Za-z0-9_])s[0-9]+([^A-Za-z0-9_]|$)/)) { t = substr(line, RSTART, RLENGTH); gsub(/[^a-z0-9]/, "", t); print t; exit }
        l = tolower(line)
        if (match(l, /spec-(author|architect|planner|implementer|reviewer)|orchestrator/)) { print substr(l, RSTART, RLENGTH); exit }
    }' "$out")
[ -z "$tok" ] || { echo "body=refused stage-ref=$tok"; exit 3; }
cat "$out"
