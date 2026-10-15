#!/bin/sh
# Assemble the tracking issue's body from a run's artifacts — the spec as
# filed, then as merged: this runs at filing, when a split is filed, and at
# gate 4 after spec-author synced spec-diff.md from the branch.
#
#   sh .claude/scripts/issue-body.sh <slug> > <out>
#
# Line 1 is `# <title>` (the orchestrator strips it for --title). Sources:
#   issue.md        title, ## Goal
#   parent card     spec rev, lane, pr, split (frontmatter) — the status line
#   spec-diff.md    ## Spec (rev N): `### Scenarios`, one item per `## S<n>`
#                   section with its Given/When/Then lines as sub-items;
#                   `### Requirements`, one table row per other
#                   `## <ID>` section (kind + `Change:` in the Kind cell; local
#                   R<n>/E<n> IDs before gate 2, as a /spec-request issue files);
#                   `### Other spec changes`, one table row per
#                   `- <file> › <section> › <change>` item; `### Deferred`,
#                   the open points and their owners (gone after the sync);
#                   ## Decisions, one item per dated `Revisions:` line
#                   (`- rev N (YYYY-MM-DD) — a; b (who)`): `- <date>, <who>`
#                   with `a`, `b` as sub-items; the gate 4 sync's own
#                   `(spec-author)` line is skipped. Only a revision made after
#                   filing carries a date, so the section is absent when the
#                   filed spec never changed; the `- decided` lines are the
#                   drafting history and stay in spec-diff.md
#   architecture.summary.md  ## Architecture: the Decide line, the component
#                   table and the public surface (absent on no-arch)
#   request.md      the reused issue's original body, collapsed (if present)
# An epic run (parent card `kind: epic`) adds the skeleton's `### Interfaces`
# from architecture.md under ## Architecture, and a ## Slices list from
# slices.md, each slice linked to its issue once the card's `slices:` has it.
#
#   sh .claude/scripts/issue-body.sh <slug> --slice <n>
#
# Slice n's own issue: title `<epic title>: <slice title>`, a status line
# `**Part of epic** #<e>` (issue-request.sh --epic reads it), the slice's
# goal, ## Spec (rev 1) holding only the scenarios and entries slices.md
# assigns it, its Deferred items, and ## Architecture with its Adds line. An
# "Other spec changes" row citing no ID goes to slice 1. A slice run's own
# body (card `kind: slice`) keeps the `**Part of epic**` line from
# slice-line.md, so a rewrite never loses the link to its epic.
# The plan is deliberately absent: the issue states the requirement, the PR
# the implementation.
# A run without spec-diff.md (non-behavior change) states "no spec change".
# The heading `## Spec (rev N)` and the `<details>` block are parsed by
# issue-request.sh and the Resume table — keep them when changing the shape.
case $# in
    1) slice= ;;
    3) [ "$2" = --slice ] || { echo "usage: $0 <slug> [--slice <n>]" >&2; exit 2; }; slice=$3 ;;
    *) echo "usage: $0 <slug> [--slice <n>]" >&2; exit 2 ;;
esac
slug=$1
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
[ -f "$a/issue.md" ] || { echo "issue-body: missing $a/issue.md" >&2; exit 1; }
nth() { printf '%s' "$1" | tr -d '[] ' | tr ',' '\n' | sed -n "${2}p"; }
# slice n of slices.md as `key<TAB>value` lines: title, goal, depends, scenarios, entries, adds, deferred
slice_field() {
    awk -v n="$1" -v k="$2" '
        /^## Slice [0-9]+: / { cur = $3; sub(/:$/, "", cur); if (cur == n && k == "title") { t = $0; sub(/^## Slice [0-9]+: /, "", t); print t } next }
        cur == n && index($0, k ": ") == 1 { print substr($0, length(k) + 3) }
    ' "$a/slices.md"
}

if [ -n "$slice" ]; then
    [ -f "$a/slices.md" ] || { echo "issue-body: missing $a/slices.md" >&2; exit 1; }
    title=$(slice_field "$slice" title)
    [ -n "$title" ] || { echo "issue-body: no slice $slice in $a/slices.md" >&2; exit 1; }
    epic=$(field issue); total=$(grep -c '^## Slice [0-9]*: ' "$a/slices.md")
    printf '%s: %s\n\n' "$(head -1 "$a/issue.md")" "$title"
    deps=; for d in $(slice_field "$slice" "Depends on" | tr ',' ' '); do
        case $d in none) ;; *) num=$(nth "$(field slices)" "$d"); deps="$deps${deps:+, }${num:+#}${num:-slice $d}" ;; esac
    done
    printf '**Part of epic** #%s · **Slice** %s of %s · **Depends on** %s\n\n' "${epic:-?}" "$slice" "$total" "${deps:-none}"
    printf '## Goal\n%s\n\n' "$(slice_field "$slice" Goal)"
    only=$(printf '%s %s' "$(slice_field "$slice" Scenarios)" "$(slice_field "$slice" Entries)" | tr ',' ' ')
else
head -1 "$a/issue.md"
echo

# --- status line
rev=; nvio=0
[ -f "$a/spec-diff.md" ] && { rev=$(sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$a/spec-diff.md"); nvio=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — violated' "$a/spec-diff.md"); }
lane=$(field lane); split=$(field split)
[ "$(field kind)" = slice ] && [ -s "$a/slice-line.md" ] && printf '%s\n\n' "$(head -1 "$a/slice-line.md")"
if [ -z "$rev" ]; then line="**Spec** none"
elif [ "$nvio" -gt 0 ]; then line="**Spec** none · **Violated** $nvio entries (rev $rev)"
else line="**Spec** rev $rev"; fi
[ -n "$lane" ] && line="$line · **Lane** $lane"
[ -n "$split" ] && line="$line · **Split off** $(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
printf '%s\n\n' "$line"

# --- goal
sed -n '/^## Goal$/,/^## /{ /^## /d; p; }' "$a/issue.md" | sed '/^$/d' | { echo "## Goal"; cat; echo; }
only=
fi

# --- spec; the same pass renders the Decisions section with part=decisions
spec_awk='
        function keep(x) { return only == "" || index(" " only " ", " " x " ") > 0 }
        function cites(line,    k, i, w) {
            if (only == "") return 1
            if (line !~ /(^|[^A-Za-z0-9])([A-Z][A-Z]-)?[RE]-?[0-9]+([^A-Za-z0-9]|$)/) return first
            k = split(only, w, " ")
            for (i = 1; i <= k; i++) if (match(line, "(^|[^A-Za-z0-9-])" w[i] "([^A-Za-z0-9]|$)")) return 1
            return 0
        }
        function cell(s) { gsub(/\|/, "\\|", s); gsub(/^[ \t]+|[ \t]+$/, "", s); return s == "" ? "—" : s }
        function flush() {
            if (id == "") return
            if (text == "" && why != "") text = why
            reqs = reqs "| " id " | " cell(kind) " | " cell(text) " |\n"
            id = ""
        }
        function revision(line,    date, who, body, k, i) {
            if (!match(line, /^- rev [0-9]+ \([0-9-]+\) — /) || line ~ /\(spec-author\)$/) return
            date = line; sub(/^- rev [0-9]+ \(/, "", date); sub(/\).*/, "", date)
            body = substr(line, RLENGTH + 1); who = "agent"
            if (match(body, / \([a-z-]+\)$/)) { who = substr(body, RSTART + 2, RLENGTH - 3); body = substr(body, 1, RSTART - 1) }
            revs = revs "- " date ", " who "\n"
            k = split(body, item, /; /)
            for (i = 1; i <= k; i++) revs = revs "  - " item[i] "\n"
        }
        /^## Summary$/ { flush(); sec = "summary"; next }
        /^## Other spec changes$/ { flush(); sec = "other"; next }
        /^## Deferred$/ { flush(); sec = "deferred"; next }
        /^## S[0-9]+ — / {
            flush(); sec = "scenario"
            if (!keep($2)) { sec = "skip"; next }
            t = $0; sub(/^## S[0-9]+ — /, "", t)
            scens = scens "- **" $2 "** — " t "\n"
            next
        }
        sec == "scenario" && /^(Given|When|Then|And) / { scens = scens "  - " $0 "\n"; next }
        sec == "deferred" && /^- / { if (only == "") defs = defs $0 "\n"; next }
        /^## / {
            flush(); sec = "entry"
            if (!keep($2)) { sec = "skip"; next }
            id = $2; kind = "new"; text = ""; why = ""
            if (match($0, / — [a-z]+/)) kind = substr($0, RSTART + 3, RLENGTH - 3)
            next
        }
        sec == "entry" && /^Change: / { kind = kind " · " substr($0, 9); next }
        sec == "entry" && /^- / { why = why (why == "" ? "" : "<br>") substr($0, 3); next }
        sec == "entry" && /^[^ ]/ { text = text (text == "" ? "" : "<br>") $0; next }
        sec == "summary" && /^- rev / { revision($0); next }
        sec == "other" && /^- / {
            if (!cites($0)) next
            line = substr($0, 3); k = split(line, f, / › /)
            others = others "| " cell(f[1]) " | " cell(k > 2 ? f[2] : "") " | " cell(k > 2 ? f[3] : f[k]) " |\n"
            next
        }
        END {
            flush()
            if (part == "decisions") { if (revs != "") printf "## Decisions\n%s\n", revs; exit }
            if (scens != "") printf "### Scenarios\n%s\n", scens
            if (reqs != "") printf "### Requirements\n| ID | Kind | Requirement |\n|---|---|---|\n%s\n", reqs
            if (others != "") printf "### Other spec changes\n| File | Section | Change |\n|---|---|---|\n%s\n", others
            if (defs != "") printf "### Deferred\n%s\n", defs
        }
'
if [ -n "$slice" ]; then
    if [ -f "$a/spec-diff.md" ]; then
        printf '## Spec (rev 1)\n\n'
        first=0; [ "$slice" = 1 ] && first=1
        awk -v part=spec -v only="$only" -v first="$first" "$spec_awk" "$a/spec-diff.md"
    fi
    defs=$(slice_field "$slice" Deferred)
    case $defs in ''|none) ;; *) printf '### Deferred\n%s\n\n' "$(printf '%s' "$defs" | tr ';' '\n' | sed 's/^ *//; s/^/- /')" ;; esac
    printf '## Architecture\nAdds: %s\n\nThe skeleton every slice builds on is in epic #%s, `## Architecture`.\n' "$(slice_field "$slice" Adds)" "${epic:-?}"
    exit 0
fi
if [ -f "$a/spec-diff.md" ]; then
    printf '## Spec (rev %s)\n\n' "$rev"
    [ "$nvio" -gt 0 ] && printf 'No spec change: the code violates the entries below.\n\n'
    awk -v part=spec "$spec_awk" "$a/spec-diff.md"
fi

# --- architecture
if [ -f "$a/architecture.summary.md" ]; then
    echo "## Architecture"
    sed -n 's/^Decide: //p' "$a/architecture.summary.md"
    echo
    sed -n '/^|/p' "$a/architecture.summary.md"
    echo
    surface=$(awk '/^Public surface:/ { on = 1; next } on && /^- / { if ($0 == "- none") next; print; next } on { on = 0 }' "$a/architecture.summary.md")
    [ -n "$surface" ] && printf 'Public surface:\n%s\n\n' "$surface"
    if [ "$(field kind)" = epic ] && [ -f "$a/architecture.md" ]; then
        printf '### Interfaces\n'
        awk '/^```/ { fence = !fence } !fence && /^## / { on = ($0 == "## Interfaces"); next } on' "$a/architecture.md" | sed 's/^### /#### /'
    fi
fi

# --- slices (epic run)
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

# --- decisions
[ -f "$a/spec-diff.md" ] && awk -v part=decisions "$spec_awk" "$a/spec-diff.md"

# --- original request
if [ -s "$a/request.md" ]; then
    echo "<details><summary>Original request</summary>"
    echo
    cat "$a/request.md"
    echo "</details>"
fi
