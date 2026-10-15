#!/bin/sh
# Print the lines of an approval file that the orchestrator quotes in its
# approval prompt — never the rest of the file. The orchestrator composes the
# prompt from this output alone (`AGENTS.workflow.md` `### Approval prompt`).
#
#   sh .claude/scripts/prompt-lines.sh <path> [<path> ...]
#
# Per file, by name:
#   spec-diff.md       Goal: · Scenarios: (count + S-IDs) · Entries: (counts + IDs, local
#                      R<n>/E<n> or fixed, or the violated list for a no-diff) ·
#                      Deferred: (count per owner)
#   architecture.summary.md  Architecture: (the Decide line) · Interfaces: · Surface: (public-surface lines)
#   plan.summary.md    Plan: (the Decide line) · Spec: (the Spec-since-architecture line) ·
#                      Stops: (stages whose Stop column says yes) · Verification:
#   review.verdict.md  Review: (the Verdict line) · Spec: · Spec changes: · Minors (…): · Needs user: — a block whose only item is `- none` is skipped
#   issue.md           Issue: <title>
#   split-issue.md     Split: <title>
#   pr.md              PR: <title>
#   question.md        the whole file (it is written to be quoted)
# then, once, over every file given: Confirm: with every `decided (chosen)`
# line of spec-diff.md (prefixed with its ID) and every `Decisions taken`
# line of architecture.summary.md and plan.summary.md, or `Confirm: nothing flagged`.
# PROMPT_SINCE_REV=<N> (the spec-diff.md rev the user last approved) keeps
# only spec-diff.md's `decided (chosen, rev M)` lines with M > N; an untagged
# `decided (chosen)` line is rev 1.
# Other files print nothing. Exit 1 on an unreadable file.
[ $# -ge 1 ] || { echo "usage: $0 <path> [<path> ...]" >&2; exit 2; }
rc=0
confirm=
nl='
'
for f in "$@"; do
    [ -r "$f" ] || { echo "prompt-lines: not readable: $f" >&2; rc=1; continue; }
    case $(basename "$f") in
    spec-diff.md)
        sed -n 's/^Goal: /Goal: /p' "$f" | head -1
        awk '/^## S[0-9]+ — / { n++; ids = ids (ids == "" ? "" : ", ") $2 } END { if (n) print "Scenarios: " n " — " ids }' "$f"
        awk '
            /^## ([A-Z][A-Z]-[RE]-[0-9]+|[RE][0-9]+) — / { k = ""; if (match($0, / — [a-z]+/)) k = substr($0, RSTART + 3, RLENGTH - 3); n[k]++; ids[k] = ids[k] (ids[k] == "" ? "" : ", ") $2; next }
            END {
                line = "Entries:"
                if (n["violated"]) line = line " no spec change — " n["violated"] " violated: " ids["violated"]
                else { sep = ""; split("new changed dropped", ks, " "); for (i = 1; i <= 3; i++) { k = ks[i]; if (n[k]) { line = line sep " " n[k] " " k " — " ids[k]; sep = " ·" } } }
                print line
            }' "$f"
        awk '/^## Deferred$/ { on = 1; next } on && /^## / { on = 0 } on && /^- / { k = "unowned"; if (match($0, / — (architecture|plan|implementation)$/)) k = substr($0, RSTART + 3); n[k]++; t++ }
            END { if (!t) exit; line = "Deferred: " t; sep = " —"; split("architecture plan implementation unowned", ks, " "); for (i = 1; i <= 4; i++) if (n[ks[i]]) { line = line sep " " n[ks[i]] " " ks[i]; sep = " ·" } print line }' "$f"
        c=$(awk -v since="${PROMPT_SINCE_REV:-0}" '/^## ([A-Z][A-Z]-[RE]-[0-9]+|[RES][0-9]+) — / { id = $2; next } /^## / { id = "" }
            /^- decided \(chosen(, rev [0-9]+)?\): / { r = 1; if (match($0, /, rev [0-9]+\)/)) r = substr($0, RSTART + 6, RLENGTH - 7) + 0; if (r <= since + 0) next; t = $0; sub(/^- decided \(chosen(, rev [0-9]+)?\): /, "", t); print "- " (id == "" ? "" : id " — ") t }' "$f")
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    architecture.summary.md)
        sed -n 's/^Decide: /Architecture: /p' "$f"
        sed -n 's/^Interfaces: /Interfaces: /p' "$f"
        awk '/^Public surface:/ { on = 1; next } on && /^- / { if ($0 == "- none") next; if (!p) { print "Surface:"; p = 1 } print; next } on { on = 0 }' "$f"
        c=$(awk '/^Decisions taken \(confirm\):/ { on = 1; next } on && /^- / { print; next } on { on = 0 }' "$f")
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    plan.summary.md)
        sed -n 's/^Decide: /Plan: /p' "$f"
        sed -n 's/^Spec since architecture: /Spec since arch: /p' "$f"
        awk -F'|' '/^\| s[0-9]/ { sid = $2; st = $6; gsub(/^ +| +$/, "", sid); gsub(/^ +| +$/, "", st); if (st ~ /^yes/) { if (!p) { print "Stops:"; p = 1 } print "- " sid " — " st } }' "$f"
        awk '/^Verification beyond unit tests:/ { on = 1; next } on && /^(- |  - )/ { if ($0 == "- none") next; if (!p) { print "Verification:"; p = 1 } print; next } on { on = 0 }' "$f"
        c=$(awk '/^Decisions taken \(confirm\):/ { on = 1; next } on && /^- / { print; next } on { on = 0 }' "$f")
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    review.verdict.md)
        sed -n 's/^Verdict: /Review: /p' "$f"
        sed -n 's/^Spec — /Spec: /p' "$f"
        sed -n 's/^Spec changes — none$//; s/^Spec changes — /Spec changes: /p' "$f"
        awk '/^Minors \(folded into / { on = 1; hdr = $0; next } on && /^- / { if ($0 == "- none") next; if (!p) { print hdr; p = 1 } print; next } on { on = 0 }' "$f"
        awk '/^Needs user/ { on = 1; next } on && /^(- |  - )/ { if ($0 == "- none") next; if (!p) { print "Needs user:"; p = 1 } print; next } on { on = 0 }' "$f" ;;
    issue.md) printf 'Issue: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    split-issue.md) printf 'Split: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    pr.md) printf 'PR: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    question.md) cat "$f" ;;
    esac
done
case " $* " in
    *spec-diff.md*|*architecture.summary.md*|*plan.summary.md*)
        if [ -n "$confirm" ]; then printf 'Confirm:\n%s' "$confirm"; else echo "Confirm: nothing flagged"; fi ;;
esac
exit $rc
