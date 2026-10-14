#!/bin/sh
# Assemble the tracking issue's body from a run's artifacts — the body is
# always the current spec, so this runs at filing and after every revision.
#
#   sh .claude/scripts/issue-body.sh <slug> > <out>
#
# Line 1 is `# <title>` (the orchestrator strips it for --title). Sources:
#   issue.md        title, ## Goal
#   parent card     spec rev, lane, pr, split (frontmatter) — the status line
#   spec-diff.md    ## Spec (rev N): `### Requirements`, one table row per
#                   `## <ID>` section (kind + `Change:` in the Kind cell);
#                   `### Decisions`, one table row per `- decided` line, the
#                   `## Summary` ones first; `### Other spec changes`, each
#                   item's trailing `: <row>` moved into a fenced block
#   plan.summary.md ## Plan: the stage table (Stage, What)
#   plan.md         ## Plan `### Decisions` from `## Shared` `Decisions:`
#   request.md      the reused issue's original body, collapsed (if present)
# A run without spec-diff.md (non-behavior change) states "no spec change".
# The heading `## Spec (rev N)` and the `<details>` block are parsed by
# issue-request.sh and the Resume table — keep them when changing the shape.
[ $# -eq 1 ] || { echo "usage: $0 <slug>" >&2; exit 2; }
slug=$1
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
[ -f "$a/issue.md" ] || { echo "issue-body: missing $a/issue.md" >&2; exit 1; }

# Shared awk: a `- decided (who): choice — rejected: alt; why` line, or the
# plan's `- choice (who) — rejected: alt; why`, as one decision-table row. Pipes inside a cell are escaped; empty cells print `—`.
decision_row='
function cell(s) { gsub(/\|/, "\\|", s); gsub(/^[ \t]+|[ \t]+$/, "", s); return s == "" ? "—" : s }
function decision(line, entry,    who, rest, chosen, rej, alt, why, i) {
    who = "—"
    if (match(line, /^- decided \([a-z-]+\): /)) {
        who = substr(line, 12, RLENGTH - 14); rest = substr(line, RLENGTH + 1)
    } else if (match(line, /^- decided: /)) {
        rest = substr(line, RLENGTH + 1)
    } else { rest = substr(line, 3) }
    rej = ""
    if ((i = index(rest, " — rejected: ")) > 0) { chosen = substr(rest, 1, i - 1); rej = substr(rest, i + 13) }
    else if ((i = index(rest, "; rejected: ")) > 0) { chosen = substr(rest, 1, i - 1); rej = substr(rest, i + 12) }
    else chosen = rest
    if (match(chosen, / \((user|chosen)\)$/)) { who = substr(chosen, RSTART + 2, RLENGTH - 3); chosen = substr(chosen, 1, RSTART - 1) }
    alt = rej; why = ""
    if ((i = index(rej, "; ")) > 0) { alt = substr(rej, 1, i - 1); why = substr(rej, i + 2) }
    sub(/\.$/, "", chosen); sub(/\.$/, "", alt); sub(/\.$/, "", why)
    return (entry == "" ? "" : "| " cell(entry) " ") "| " cell(chosen) " | " cell(alt) " | " cell(why) " | " who " |"
}'

head -1 "$a/issue.md"
echo

# --- status line
rev=; nvio=0
[ -f "$a/spec-diff.md" ] && { rev=$(sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$a/spec-diff.md"); nvio=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — violated' "$a/spec-diff.md"); }
lane=$(field lane); pr=$(field pr); split=$(field split)
if [ -z "$rev" ]; then line="**Spec** none"
elif [ "$nvio" -gt 0 ]; then line="**Spec** none · **Violated** $nvio entries (rev $rev)"
else line="**Spec** rev $rev"; fi
[ -n "$lane" ] && line="$line · **Lane** $lane"
[ -n "$pr" ] && line="$line · **PR** #$pr"
[ -n "$split" ] && line="$line · **Split off** $(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
printf '%s\n\n' "$line"

# --- goal
sed -n '/^## Goal$/,/^## /{ /^## /d; p; }' "$a/issue.md" | sed '/^$/d' | { echo "## Goal"; cat; echo; }

# --- spec
if [ -f "$a/spec-diff.md" ]; then
    area=$(sed -n 's/^Area: \([^ ]*\).*/\1/p' "$a/spec-diff.md" | head -1)
    printf '## Spec (rev %s)%s\n\n' "$rev" "${area:+ — $area}"
    [ "$nvio" -gt 0 ] && printf 'No spec change: the code violates the entries below.\n\n'
    awk "$decision_row"'
        function flush() {
            if (id == "") return
            if (text == "" && why != "") text = why
            reqs = reqs "| " id " | " cell(kind) " | " cell(text) " |\n"
            id = ""
        }
        /^## Summary$/ { flush(); sec = "summary"; next }
        /^## Other spec changes$/ { flush(); sec = "other"; next }
        /^## / {
            flush(); sec = "entry"
            id = $2; kind = "new"; text = ""; why = ""
            if (match($0, / — [a-z]+/)) kind = substr($0, RSTART + 3, RLENGTH - 3)
            next
        }
        sec == "entry" && /^Change: / { kind = kind " · " substr($0, 9); next }
        sec == "entry" && /^- decided/ { decs = decs decision($0, id) "\n"; next }
        sec == "entry" && /^- / { why = why (why == "" ? "" : "<br>") substr($0, 3); next }
        sec == "entry" && /^[^ ]/ { text = text (text == "" ? "" : "<br>") $0; next }
        sec == "summary" && /^- decided/ { sdecs = sdecs decision($0, "—") "\n"; next }
        sec == "other" && /^- / {
            line = substr($0, 3); row = ""
            # The row is the backtick-wrapped tail after the last ": `".
            for (i = length(line); i > 2; i--)
                if (substr(line, i - 2, 3) == ": `") { row = substr(line, i); line = substr(line, 1, i - 3); break }
            gsub(/^`+ ?| ?`+$/, "", row)
            others = others "- " line (row == "" ? "" : ":\n  ```\n  " row "\n  ```") "\n"
            next
        }
        END {
            flush()
            if (reqs != "") printf "### Requirements\n| ID | Kind | Requirement |\n|---|---|---|\n%s\n", reqs
            if (sdecs decs != "") printf "### Decisions\n| Entry | Chosen | Rejected | Why | By |\n|---|---|---|---|---|\n%s%s\n", sdecs, decs
            if (others != "") printf "### Other spec changes\n%s\n", others
        }
    ' "$a/spec-diff.md"
fi

# --- plan
stages=; dec=
[ -f "$a/plan.summary.md" ] && stages=$(awk -F'|' '/^\| s[0-9]/ { gsub(/^ +| +$/, "", $2); gsub(/^ +| +$/, "", $3); printf "| %s | %s |\n", $2, $3 }' "$a/plan.summary.md")
[ -f "$a/plan.md" ] && dec=$(sh "$root/.claude/scripts/extract-section.sh" '## Shared' "$a/plan.md" 2>/dev/null | awk "$decision_row"'/^Decisions:/ { on = 1; next } on && /^- / { print decision($0, ""); next } on && !/^- / && !/^$/ { on = 0 }')
if [ -n "$stages$dec" ]; then
    printf '## Plan\n\n'
    [ -n "$stages" ] && printf '| Stage | Title |\n|---|---|\n%s\n\n' "$stages"
    [ -n "$dec" ] && printf '### Decisions\n| Chosen | Rejected | Why | By |\n|---|---|---|---|\n%s\n\n' "$dec"
fi

# --- original request
if [ -f "$a/request.md" ]; then
    echo "<details><summary>Original request</summary>"
    echo
    cat "$a/request.md"
    echo "</details>"
fi
