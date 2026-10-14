#!/bin/sh
# Print the lines of an approval file that the orchestrator quotes in its
# approval prompt — never the rest of the file. The orchestrator composes the
# prompt from this output alone (`AGENTS.workflow.md` `### Approval prompt`).
#
#   sh .claude/scripts/prompt-lines.sh <path> [<path> ...]
#
# Per file, by name:
#   spec-diff.md       Goal: · Entries: (counts + IDs, or the violated list for a no-diff)
#   plan.summary.md    Plan: (the Decide line) · Stops: (stages whose Stop column says yes) · Verification:
#   review.verdict.md  Review: (the Verdict line) · Spec: · Minors (…): · Needs user: — a block whose only item is `- none` is skipped
#   issue.md           Issue: <title>
#   split-issue.md     Split: <title>
#   pr.md              PR: <title>
#   merge-message.txt  Merge: <subject>
#   question.md        the whole file (it is written to be quoted)
# then, once, over every file given: Confirm: with every `decided (chosen)`
# line of spec-diff.md (prefixed with its ID) and every `Decisions taken`
# line of plan.summary.md, or `Confirm: nothing flagged`.
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
        awk '
            /^## [A-Z][A-Z]-[RE]-[0-9]+ — / { k = ""; if (match($0, / — [a-z]+/)) k = substr($0, RSTART + 3, RLENGTH - 3); n[k]++; ids[k] = ids[k] (ids[k] == "" ? "" : ", ") $2; next }
            END {
                line = "Entries:"
                if (n["violated"]) line = line " no spec change — " n["violated"] " violated: " ids["violated"]
                else { sep = ""; split("new changed dropped", ks, " "); for (i = 1; i <= 3; i++) { k = ks[i]; if (n[k]) { line = line sep " " n[k] " " k " — " ids[k]; sep = " ·" } } }
                print line
            }' "$f"
        c=$(awk '/^## [A-Z][A-Z]-[RE]-[0-9]+ — / { id = $2; next } /^## / { id = "" } /^- decided \(chosen\)/ { print "- " (id == "" ? "" : id " — ") substr($0, 21) }' "$f")
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    plan.summary.md)
        sed -n 's/^Decide: /Plan: /p' "$f"
        awk -F'|' '/^\| s[0-9]/ { sid = $2; st = $6; gsub(/^ +| +$/, "", sid); gsub(/^ +| +$/, "", st); if (st ~ /^yes/) { if (!p) { print "Stops:"; p = 1 } print "- " sid " — " st } }' "$f"
        awk '/^Verification beyond unit tests:/ { on = 1; next } on && /^- / { if (!p) { print "Verification:"; p = 1 } if ($0 != "- none") print; next } on { on = 0 }' "$f"
        c=$(awk '/^Decisions taken \(confirm\):/ { on = 1; next } on && /^- / { print; next } on { on = 0 }' "$f")
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    review.verdict.md)
        sed -n 's/^Verdict: /Review: /p' "$f"
        sed -n 's/^Spec — /Spec: /p' "$f"
        awk '/^Minors \(folded into / { on = 1; hdr = $0; next } on && /^- / { if ($0 == "- none") next; if (!p) { print hdr; p = 1 } print; next } on { on = 0 }' "$f"
        awk '/^Needs user/ { on = 1; next } on && /^(- |  - )/ { if ($0 == "- none") next; if (!p) { print "Needs user:"; p = 1 } print; next } on { on = 0 }' "$f" ;;
    issue.md) printf 'Issue: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    split-issue.md) printf 'Split: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    pr.md) printf 'PR: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    merge-message.txt) printf 'Merge: %s\n' "$(head -1 "$f")" ;;
    question.md) cat "$f" ;;
    esac
done
case " $* " in
    *spec-diff.md*|*plan.summary.md*)
        if [ -n "$confirm" ]; then printf 'Confirm:\n%s' "$confirm"; else echo "Confirm: nothing flagged"; fi ;;
esac
exit $rc
