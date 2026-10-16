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
#   slices.md          Slices: one line per slice — number, title, depends on, scenarios, entries;
#                      Confirm: every S/R/E of the sibling spec-diff.md that no slice or two slices claim
#   plan.summary.md    Plan: (the Decide line) · Spec: (the Spec-since-architecture line) ·
#                      Stops: (stages whose Stop column says yes) · Verification:
#   review.verdict.md  Review: (the Verdict line) · Not checked: · Spec: · Spec changes: · Minors (…): · Needs user: — a block whose only item is `- none` is skipped
#   issue.md           Issue: <title>
#   split-issue.md     Split: <title>
#   follow-up-<k>.md   Follow-up <k>: <title>
#   pr.md              PR: <title>
#   question.md        the whole file (it is written to be quoted)
# then, once, over every file given: Confirm: with every `decided (chosen)`
# line of spec-diff.md (prefixed with its ID) and every `Decisions taken`
# line of architecture.summary.md and plan.summary.md, or `Confirm: nothing flagged`.
# PROMPT_SINCE_REV=<N> (the spec-diff.md rev the user last approved) keeps
# only spec-diff.md's `decided (chosen, rev M)` lines with M > N; an untagged
# `decided (chosen)` line is rev 1.
# A `[spec <ID>]` decision line of the sibling architecture.md (`## Decisions`)
# or plan.md (`## Shared` `Decisions:`), `- <choice> (<who>[, rev M]) [spec <ID>]
# — <why>`, joins that Confirm: as `- <ID> — <choice> (<who>) — <why>`, unless
# M <= PROMPT_SINCE_REV; design and process lines are never quoted.
# Other files print nothing. Exit 1 on an unreadable file.
[ $# -ge 1 ] || { echo "usage: $0 <path> [<path> ...]" >&2; exit 2; }
rc=0
confirm=
nl='
'
# stdin: decision lines; prints the [spec <ID>] ones the user has not yet approved
spec_decisions() {
    awk -v since="${PROMPT_SINCE_REV:-0}" '
        !match($0, /\) \[spec [A-Z0-9-]+\]/) { next }
        {
            head = substr($0, 1, RSTART - 1); id = substr($0, RSTART + 8, RLENGTH - 9)
            rest = substr($0, RSTART + RLENGTH); sub(/^ — /, "", rest)
            who = head; sub(/.* \(/, "", who); choice = head; sub(/ \([^(]*$/, "", choice); sub(/^- /, "", choice)
            r = 0; if (match(who, /, rev [0-9]+$/)) { r = substr(who, RSTART + 6) + 0; sub(/, rev [0-9]+$/, "", who) }
            if (r > 0 && r <= since + 0) next
            print "- " id " — " choice " (" who ") — " rest
        }'
}
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
        [ -n "$c" ] && confirm="$confirm$c$nl"
        c=$(awk '/^## Decisions$/ { on = 1; next } /^## / { on = 0 } on' "$(dirname -- "$f")/architecture.md" 2>/dev/null | spec_decisions)
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    slices.md)
        awk '/^## Slice [0-9]+: / { if (t != "") print t; t = $0; sub(/^## Slice /, "- ", t); next }
            /^Depends on: / { t = t " — depends on " substr($0, 13) } /^Scenarios: / { t = t " — " substr($0, 12) }
            /^Entries: / { t = t " · " substr($0, 10) }
            END { if (t != "") print t }' "$f" | { read -r first && { echo "Slices:"; printf '%s\n' "$first"; cat; }; }
        sd=$(dirname -- "$f")/spec-diff.md
        if [ -f "$sd" ]; then
            c=$(awk '
                FNR == 1 { file++ }
                file == 1 && /^## (S[0-9]+|[RE][0-9]+|[A-Z][A-Z]-[RE]-[0-9]+) — / && $0 !~ / — (dropped|violated)/ { ids[$2] = 1; order[++n] = $2 }
                file == 2 && /^## Slice [0-9]+: / { s = $3; sub(/:$/, "", s) }
                file == 2 && /^(Scenarios|Entries): / { line = $0; sub(/^[A-Za-z]+: /, "", line); k = split(line, w, /, */)
                    for (i = 1; i <= k; i++) if (w[i] != "" && w[i] != "none") { cnt[w[i]]++; where[w[i]] = where[w[i]] (where[w[i]] == "" ? "" : ", ") s } }
                END { for (i = 1; i <= n; i++) { id = order[i]
                        if (!cnt[id]) print "- slices: " id " is in no slice"
                        else if (cnt[id] > 1) print "- slices: " id " is in slices " where[id] } }' "$sd" "$f")
            [ -n "$c" ] && confirm="$confirm$c$nl"
        fi ;;
    plan.summary.md)
        sed -n 's/^Decide: /Plan: /p' "$f"
        sed -n 's/^Spec since architecture: /Spec since arch: /p' "$f"
        awk -F'|' '/^\| s[0-9]/ { sid = $2; st = $6; gsub(/^ +| +$/, "", sid); gsub(/^ +| +$/, "", st); if (st ~ /^yes/) { if (!p) { print "Stops:"; p = 1 } print "- " sid " — " st } }' "$f"
        awk '/^Verification beyond unit tests:/ { on = 1; next } on && /^(- |  - )/ { if ($0 == "- none") next; if (!p) { print "Verification:"; p = 1 } print; next } on { on = 0 }' "$f"
        c=$(awk '/^Decisions taken \(confirm\):/ { on = 1; next } on && /^- / { print; next } on { on = 0 }' "$f")
        [ -n "$c" ] && confirm="$confirm$c$nl"
        c=$(awk '/^## Shared$/ { sh = 1; next } /^## / { sh = 0 } sh && /^Decisions:/ { on = 1; next } on && /^- / { print; next } on { on = 0 }' "$(dirname -- "$f")/plan.md" 2>/dev/null | spec_decisions)
        [ -n "$c" ] && confirm="$confirm$c$nl" ;;
    review.verdict.md)
        sed -n 's/^Verdict: /Review: /p' "$f"
        sed -n 's/^Not checked — /Not checked: /p' "$f"
        sed -n 's/^Spec — /Spec: /p' "$f"
        sed -n 's/^Spec changes — none$//; s/^Spec changes — /Spec changes: /p' "$f"
        awk '/^Minors \(folded into / { on = 1; hdr = $0; next } on && /^- / { if ($0 == "- none") next; if (!p) { print hdr; p = 1 } print; next } on { on = 0 }' "$f"
        awk '/^Needs user/ { on = 1; next } on && /^(- |  - )/ { if ($0 == "- none") next; if (!p) { print "Needs user:"; p = 1 } print; next } on { on = 0 }' "$f" ;;
    issue.md) printf 'Issue: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    split-issue.md) printf 'Split: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    follow-up-*.md) k=$(basename "$f" .md); printf 'Follow-up %s: %s\n' "${k#follow-up-}" "$(head -1 "$f" | sed 's/^# //')" ;;
    pr.md) printf 'PR: %s\n' "$(head -1 "$f" | sed 's/^# //')" ;;
    question.md) cat "$f" ;;
    esac
done
case " $* " in
    *spec-diff.md*|*architecture.summary.md*|*plan.summary.md*|*slices.md*)
        if [ -n "$confirm" ]; then printf 'Confirm:\n%s' "$confirm"; else echo "Confirm: nothing flagged"; fi ;;
esac
exit $rc
