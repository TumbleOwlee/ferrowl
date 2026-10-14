#!/bin/sh
# Assemble the tracking issue's body from a run's artifacts — the body is
# always the current spec, so this runs at filing and after every revision.
#
#   sh .claude/scripts/issue-body.sh <slug> > <out>
#
# Line 1 is `# <title>` (the orchestrator strips it for --title). Sources:
#   issue.md        title, ## Goal
#   parent card     spec rev, lane, pr, split (frontmatter) — the status line
#   spec-diff.md    ## Spec: one list item per `## <ID>` section, its
#                   `Change:` and `- decided` lines as sub-items, the
#                   `## Other spec changes` items appended
#   plan.md         ## Shared `Decisions:` → ## Plan decisions
#   plan.summary.md stage count
#   request.md      the reused issue's original body, collapsed (if present)
# A run without spec-diff.md (non-behavior change) states "no spec change".
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
lane=$(field lane); pr=$(field pr); split=$(field split); stages=
[ -f "$a/plan.summary.md" ] && stages=$(grep -c '^| s[0-9]' "$a/plan.summary.md")
line=
if [ -z "$rev" ]; then line="No spec change"
elif [ "$nvio" -gt 0 ]; then line="No spec change — $nvio entries violated (rev $rev)"
else line="Spec rev $rev"; fi
[ -n "$lane" ] && line="$line · lane $lane"
[ -n "$stages" ] && [ "$stages" -gt 0 ] && line="$line · $stages stages"
[ -n "$pr" ] && line="$line · PR #$pr"
[ -n "$split" ] && line="$line · split off: $(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
printf '%s\n\n' "$line"

# --- goal
sed -n '/^## Goal$/,/^## /{ /^## /d; p; }' "$a/issue.md" | sed '/^$/d' | { echo "## Goal"; cat; echo; }

# --- spec
if [ -f "$a/spec-diff.md" ]; then
    area=$(sed -n 's/^Area: \([^ ]*\).*/\1/p' "$a/spec-diff.md" | head -1)
    printf '## Spec (rev %s)%s\n' "$rev" "${area:+ — $area}"
    [ "$nvio" -gt 0 ] && echo "No spec change: the code violates the entries below."
    awk '
        BEGIN { first = 1 }
        /^## Summary$/ { sec = "summary"; next }
        /^## Other spec changes$/ { sec = "other"; next }
        /^## / {
            sec = "entry"; pending = ""
            id = $2; kind = ""
            if (match($0, / — [a-z]+/)) kind = substr($0, RSTART + 3, RLENGTH - 3)
            label = "- **" id "**" (kind == "new" ? "" : " " kind) " — "
            next
        }
        sec == "entry" && /^Change: / { pending = "  - change: " substr($0, 9); next }
        sec == "entry" && /^- / { if (label != "") { sub(/ — $/, "", label); print label; label = "" } print "  " $0; next }
        sec == "entry" && /^[^ ]/ { print label $0; label = ""; if (pending != "") { print pending; pending = "" } next }
        sec == "summary" && /^- decided/ { print; next }
        sec == "other" && /^- / { print; next }
    ' "$a/spec-diff.md"
    echo
fi

# --- plan decisions
if [ -f "$a/plan.md" ]; then
    dec=$(sh "$root/.claude/scripts/extract-section.sh" '## Shared' "$a/plan.md" 2>/dev/null | awk '/^Decisions:/ { on = 1; next } on && /^- / { print; next } on && !/^- / && !/^$/ { on = 0 }')
    [ -n "$dec" ] && { echo "## Plan decisions"; printf '%s\n\n' "$dec"; }
fi

# --- original request
if [ -f "$a/request.md" ]; then
    echo "<details><summary>Original request</summary>"
    echo
    cat "$a/request.md"
    echo "</details>"
fi
