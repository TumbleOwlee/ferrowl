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
# The plan is deliberately absent: the issue states the requirement, the PR
# the implementation.
# A run without spec-diff.md (non-behavior change) states "no spec change".
# The heading `## Spec (rev N)` and the `<details>` block are parsed by
# issue-request.sh and the Resume table — keep them when changing the shape.
[ $# -eq 1 ] || { echo "usage: $0 <slug>" >&2; exit 2; }
slug=$1
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
[ -f "$a/issue.md" ] || { echo "issue-body: missing $a/issue.md" >&2; exit 1; }

head -1 "$a/issue.md"
echo

# --- status line
rev=; nvio=0
[ -f "$a/spec-diff.md" ] && { rev=$(sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$a/spec-diff.md"); nvio=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — violated' "$a/spec-diff.md"); }
lane=$(field lane); split=$(field split)
if [ -z "$rev" ]; then line="**Spec** none"
elif [ "$nvio" -gt 0 ]; then line="**Spec** none · **Violated** $nvio entries (rev $rev)"
else line="**Spec** rev $rev"; fi
[ -n "$lane" ] && line="$line · **Lane** $lane"
[ -n "$split" ] && line="$line · **Split off** $(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
printf '%s\n\n' "$line"

# --- goal
sed -n '/^## Goal$/,/^## /{ /^## /d; p; }' "$a/issue.md" | sed '/^$/d' | { echo "## Goal"; cat; echo; }

# --- spec; the same pass renders the Decisions section with part=decisions
spec_awk='
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
            t = $0; sub(/^## S[0-9]+ — /, "", t)
            scens = scens "- **" $2 "** — " t "\n"
            next
        }
        sec == "scenario" && /^(Given|When|Then|And) / { scens = scens "  - " $0 "\n"; next }
        sec == "deferred" && /^- / { defs = defs $0 "\n"; next }
        /^## / {
            flush(); sec = "entry"
            id = $2; kind = "new"; text = ""; why = ""
            if (match($0, / — [a-z]+/)) kind = substr($0, RSTART + 3, RLENGTH - 3)
            next
        }
        sec == "entry" && /^Change: / { kind = kind " · " substr($0, 9); next }
        sec == "entry" && /^- / { why = why (why == "" ? "" : "<br>") substr($0, 3); next }
        sec == "entry" && /^[^ ]/ { text = text (text == "" ? "" : "<br>") $0; next }
        sec == "summary" && /^- rev / { revision($0); next }
        sec == "other" && /^- / {
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
fi

# --- decisions
[ -f "$a/spec-diff.md" ] && awk -v part=decisions "$spec_awk" "$a/spec-diff.md"

# --- original request
if [ -f "$a/request.md" ]; then
    echo "<details><summary>Original request</summary>"
    echo
    cat "$a/request.md"
    echo "</details>"
fi
