#!/bin/sh
# Render one part of a run's spec-diff.md as markdown; the issue and PR bodies
# both build on it.
#
#   sh spec-render.sh <spec-diff.md> scenarios [--only "<ids>"]    # ## Scenarios list, Given/When/Then sub-items
#   sh spec-render.sh <spec-diff.md> requirements [--only "<ids>"] # ## Requirements: `- **R1** <text>` per entry, no kind/table
#   sh spec-render.sh <spec-diff.md> entries [--only "<ids>"] [--first 0|1]  # ### Requirements table + ### Other spec changes
#   sh spec-render.sh <spec-diff.md> entries --issue-ids <card-dir>
#                                   # PR form: `| ID | Issue | Kind | Requirement |`, Issue = `R<n>` from `· was R<n>`;
#                                   # Kind gains `, reworded after filing` when a card of the run holds a `spec-delta <ID> changed` line with an `origin=` field (any field order)
#   sh spec-render.sh <spec-diff.md> changes                       # dated Revisions lines ending (user), (review) or (impl)
#   sh spec-render.sh <spec-diff.md> deferred                      # ### Deferred list
#
# Each part prints its heading (changes: none), its block and one trailing
# blank line; nothing for an empty part. `--only` keeps the scenarios and
# entries named (local or fixed ID); an "Other spec changes" row citing no ID
# is kept only with `--first 1`. In `scenarios` and `requirements` a fixed ID
# with a `· was R<n>` heading is written back as its local ID.
# Exit 2 on usage, 1 on a missing file.
usage() { echo "usage: $0 <spec-diff.md> scenarios|requirements|entries|changes|deferred [--only \"<ids>\"] [--first 0|1] [--issue-ids <card-dir>]" >&2; exit 2; }
[ $# -ge 2 ] || usage
file=$1; part=$2; shift 2
case $part in scenarios|requirements|entries|changes|deferred) ;; *) usage ;; esac
only=; first=0; cards=
while [ $# -gt 0 ]; do
    case $1 in
        --only) [ $# -ge 2 ] || usage; only=$2; shift 2 ;;
        --first) [ $# -ge 2 ] || usage; case $2 in 0|1) first=$2 ;; *) usage ;; esac; shift 2 ;;
        --issue-ids) [ $# -ge 2 ] || usage; cards=$2; shift 2 ;;
        *) usage ;;
    esac
done
[ -f "$file" ] || { echo "spec-render: missing $file" >&2; exit 1; }

deltas=
if [ -n "$cards" ]; then
    slug=$(basename "$(cd "$(dirname "$file")" && pwd)")
    deltas=$(cat "$cards"/*/"$slug".md "$cards"/*/"$slug".s*.md 2>/dev/null | awk '$2 == "spec-delta" && $4 == "changed" {
        rest = $0; sub(/^[^ ]+ spec-delta [^ ]+ [^ ]+/, "", rest)
        while (match(rest, /^ +(origin|quote)=/)) {
            field = substr(rest, 1, RLENGTH)
            rest = substr(rest, RLENGTH + 1)
            if (substr(rest, 1, 1) == "\"") { rest = substr(rest, 2); i = index(rest, "\""); rest = substr(rest, i + 1) }
            else { i = index(rest, " "); if (i == 0) i = length(rest) + 1; rest = substr(rest, i) }
            if (field ~ /origin=$/) { print $3; break }
        }
    }' | tr '\n' ' ')
fi

spec_awk='
        function keep(x) { return only == "" || index(" " only " ", " " x " ") > 0 || ((x in map) && index(" " only " ", " " map[x] " ") > 0) }
        function cites(line,    k, i, w) {
            if (only == "") return 1
            if (line !~ /(^|[^A-Za-z0-9])([A-Z][A-Z]-)?[RE]-?[0-9]+([^A-Za-z0-9]|$)/) return first
            k = split(only, w, " ")
            for (i = 1; i <= k; i++) if (match(line, "(^|[^A-Za-z0-9-])" w[i] "([^A-Za-z0-9]|$)")) return 1
            return 0
        }
        function cell(s) { gsub(/\|/, "\\|", s); gsub(/^[ \t]+|[ \t]+$/, "", s); return s == "" ? "—" : s }
        function back(s,    out, id) {
            out = ""
            while (match(s, /[A-Z][A-Z]-[RE]-[0-9]+/)) {
                id = substr(s, RSTART, RLENGTH)
                out = out substr(s, 1, RSTART - 1) ((id in map) ? map[id] : id)
                s = substr(s, RSTART + RLENGTH)
            }
            return out s
        }
        function flush(    shown, line, k) {
            if (id == "") return
            if (text == "" && why != "") { text = why; textsp = whysc }
            if (issue_form) {
                k = cell(kind); if ((" " deltas " ") ~ (" " id " ")) k = k ", reworded after filing"
                reqs = reqs "| " id " | " ((id in map) ? map[id] : "—") " | " k " | " cell(text) " |\n"
            } else reqs = reqs "| " id " | " cell(kind) " | " cell(text) " |\n"
            shown = (id in map) ? map[id] : id
            if (base == "dropped") line = "(dropped) " whysc
            else if (base == "changed") line = "(changed) " textsp
            else line = textsp
            if (base == "violated") nvio++
            rl = rl "- **" shown "** " back(line) "\n"
            id = ""
        }
        function revision(line,    body, who, k, i, it, idr) {
            if (!match(line, /^- rev [0-9]+ \([0-9-]+\) — /)) return
            body = substr(line, RSTART + RLENGTH)
            if (!match(body, / \((user|review|impl)\)$/)) return
            who = substr(body, RSTART + 2, RLENGTH - 3); body = substr(body, 1, RSTART - 1)
            k = split(body, it, /; /)
            for (i = 1; i <= k; i++) {
                if (match(it[i], /^([A-Z][A-Z]-[RE]-[0-9]+|S[0-9]+): /)) {
                    idr = substr(it[i], 1, RLENGTH - 2)
                    chg = chg "- " idr " — " substr(it[i], RLENGTH + 1) " — " who "\n"
                } else chg = chg "- " it[i] " — " who "\n"
            }
        }
        FNR == NR {
            if ($0 ~ /^## / && match($0, / · was [RE][0-9]+$/)) { v = substr($0, RSTART); sub(/.* was /, "", v); map[$2] = v }
            next
        }
        /^## Summary$/ { flush(); sec = "summary"; next }
        /^## Other spec changes$/ { flush(); sec = "other"; next }
        /^## Deferred$/ { flush(); sec = "deferred"; next }
        /^## S[0-9]+ — / {
            flush(); sec = "scenario"
            if (!keep($2)) { sec = "skip"; next }
            t = $0; sub(/^## S[0-9]+ — /, "", t)
            scens = scens "- **" $2 "** — " back(t) "\n"
            next
        }
        sec == "scenario" && /^(Given|When|Then|And) / { scens = scens "  - " back($0) "\n"; next }
        sec == "deferred" && /^- / { defs = defs $0 "\n"; next }
        /^## / {
            flush(); sec = "entry"
            if (!keep($2)) { sec = "skip"; next }
            id = $2; kind = ($3 == "—" && $4 != "") ? $4 : "new"; base = kind; text = ""; textsp = ""; why = ""; whysc = ""
            next
        }
        sec == "entry" && /^Change: / { kind = kind " · " substr($0, 9); next }
        sec == "entry" && /^- / { why = why (why == "" ? "" : "<br>") substr($0, 3); whysc = whysc (whysc == "" ? "" : "; ") substr($0, 3); next }
        sec == "entry" && /^[^ ]/ { text = text (text == "" ? "" : "<br>") $0; textsp = textsp (textsp == "" ? "" : " ") $0; next }
        sec == "summary" && /^- rev / { revision($0); next }
        sec == "other" && /^- / {
            if (!cites($0)) next
            line = substr($0, 3); k = split(line, f, / › /)
            others = others "| " cell(f[1]) " | " cell(k > 2 ? f[2] : "") " | " cell(k > 2 ? f[3] : f[k]) " |\n"
            next
        }
        END {
            flush()
            if (part == "scenarios") { if (scens != "") printf "## Scenarios\n%s\n", scens }
            else if (part == "requirements") {
                if (rl != "") printf "## Requirements\n%s%s\n", (nvio > 0 ? "No spec change: the code violates the requirements below.\n\n" : ""), rl
            } else if (part == "entries") {
                if (reqs != "") printf "%s| ID | %sKind | Requirement |\n|---|%s---|---|\n%s\n", (issue_form ? "## Requirements\n" : "### Requirements\n"), (issue_form ? "Issue | " : ""), (issue_form ? "---|" : ""), reqs
                if (others != "") printf "### Other spec changes\n| File | Section | Change |\n|---|---|---|\n%s\n", others
            } else if (part == "changes") { if (chg != "") printf "%s\n", chg }
            else if (part == "deferred") { if (defs != "") printf "### Deferred\n%s\n", defs }
        }
'
awk -v part="$part" -v only="$only" -v first="$first" -v issue_form="$([ -n "$cards" ] && echo 1 || echo 0)" -v deltas="$deltas" "$spec_awk" "$file" "$file"
