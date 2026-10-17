#!/bin/sh
# Assemble the PR body from a run's artifacts and cards: the how, where the
# issue holds the what.
#
#   sh .claude/scripts/pr-body.sh <slug> draft > <out>    # from the first push to gate 4; rerun on every card move
#   sh .claude/scripts/pr-body.sh <slug> full  > <out>    # at gate 4, from spec-author's pr.md
#
# Line 1 is `# <title>` (stripped for --title). Both forms end with
# `Closes #<issue>` when the parent card carries `issue`.
#
# Both forms share, in order after their own opening, ## Scenarios (full:
# ## Scenarios covered), ## Requirements (with the Issue column), ## Spec
# changes since filing, ## Architecture (the Decide line, the component table
# of architecture.md, the public surface; no interfaces) and ## Decisions
# (design choices and the chosen spec decisions not already under the spec
# changes).
#
# draft:  status line (lane, spec rev, last green sha), ## Why, ## Scenarios,
#         ## Stages (a stage's verification beyond unit tests as indented
#         sub-items under its line), ## Review here, then the shared sections.
#         Stage ids live only in HTML comments on the stage lines,
#         `<!-- s1 blocked-by s0 -->`. Its spec changes are the cards'
#         spec-delta lines whose `**<ID>**` the committed branch diff
#         (`git diff main...<branch> -- docs/specs`) touches; the section is
#         omitted when the card has no `branch` or git fails.
# full:   status line (spec rev, stages, entries, coverage, review, splits),
#         pr.md from line 2 on, then the shared sections; its spec changes
#         come from the Revisions lines of the synced spec-diff.md.
#
# Exit 3 with `body=refused stage-ref=<token>` when the visible text names a
# stage (`s3`) or an agent. HTML comments, fenced blocks, code spans holding
# `.` or `/` and the original request are not scanned.
[ $# -eq 2 ] || { echo "usage: $0 <slug> draft|full" >&2; exit 2; }
slug=$1; form=$2
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
board=$root/.claude/tasks
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
sr() { sh "$root/.claude/scripts/spec-render.sh" "$a/spec-diff.md" "$@"; }
trim() { sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'; }
[ -f "$a/issue.md" ] || { echo "pr-body: missing $a/issue.md" >&2; exit 1; }
case $form in draft|full) ;; *) echo "usage: $0 <slug> draft|full" >&2; exit 2 ;; esac

issue=$(field issue); lane=$(field lane); split=$(field split)
rev=; [ -f "$a/spec-diff.md" ] && rev=$(sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$a/spec-diff.md")
spec_tok="no spec change"; [ -n "$rev" ] && spec_tok="spec rev $rev"
Spec_tok="No spec change"; [ -n "$rev" ] && Spec_tok="Spec rev $rev"
stages=0; [ -f "$a/plan.summary.md" ] && stages=$(grep -c '^| s[0-9]' "$a/plan.summary.md")
if [ -n "$issue" ]; then ref="#$issue"; else ref="the tracking issue"; fi
# "<count><TAB>S1 title · S2 title …" of spec-diff.md's scenarios
scenarios=
[ -f "$a/spec-diff.md" ] && scenarios=$(awk '/^## S[0-9]+ — / { n++; t = $0; sub(/^## /, "", t); sub(/ — /, " ", t); s = s (s == "" ? "" : " · ") t } END { if (n) print n "\t" s }' "$a/spec-diff.md")
# plan.summary.md stage table: the What column of one stage id
what_of() { awk -F'|' -v s="$1" '/^\| s[0-9]/ { id = $2; gsub(/ /, "", id); if (id == s) { w = $3; gsub(/^ +| +$/, "", w); print w; exit } }' "$a/plan.summary.md"; }

draft_head() {
    head -1 "$a/issue.md"
    echo
    sha=$(field pass-sha)
    line="Draft"
    [ -n "$lane" ] && line="$line · lane $lane"
    line="$line · $spec_tok"
    [ -n "$sha" ] && line="$line · gauntlet green at $sha"
    printf '%s\n\n' "$line"
    echo "## Why"
    sed -n '/^## Goal$/{ n; p; q; }' "$a/issue.md"
    echo
}

# Decide line, component table of architecture.md, public surface; no interfaces
architecture() {
    [ -f "$a/architecture.summary.md" ] || return 0
    printf '## Architecture\n'
    sed -n 's/^Decide: //p' "$a/architecture.summary.md"
    echo
    if [ -f "$a/architecture.md" ]; then
        tbl=$(awk '/^```/ { f = !f } !f && /^## / { on = ($0 == "## Components"); next } on && /^\|/' "$a/architecture.md")
        [ -n "$tbl" ] && printf '%s\n\n' "$tbl"
    fi
    surface=$(awk '/^Public surface:/ { on = 1; next } on && /^- / { if ($0 == "- none") next; print; next } on { on = 0 }' "$a/architecture.summary.md")
    [ -n "$surface" ] && printf 'Public surface:\n%s\n' "$surface"
}

requirements() {
    [ -f "$a/spec-diff.md" ] || return 0
    req=$(sr entries --issue-ids "$board" | trim)
    [ -n "$req" ] && printf '%s\n\n' "$req"
}

# $1: the change lines; sets chg_ids (call directly, never inside $(…))
spec_changes() {
    chg_ids=
    [ -n "$1" ] && [ -f "$a/spec-diff.md" ] || return 0
    chg_ids=$(printf '%s\n' "$1" | sed -n 's/^- \([A-Z][A-Z]-[RE]-[0-9][0-9]*\) .*/\1/p' | tr '\n' ' ')
    printf '## Spec changes since filing\n'
    {
        awk '/^## / && match($0, / · was [RE][0-9]+$/) { v = substr($0, RSTART); sub(/.* was /, "", v); print "M " $2 " " v }' "$a/spec-diff.md"
        cat "$board"/*/"$slug".md "$board"/*/"$slug".s*.md 2>/dev/null | sed -n 's/.* spec-delta \([^ ]*\) .*quote="\([^"]*\)".*/Q \1 \2/p'
        printf '%s\n' "$1" | sed 's/^/L /'
    } | awk '
        $1 == "M" { m[$2] = $3; next }
        $1 == "Q" { q = $0; sub(/^Q [^ ]+ /, "", q); qt[$2] = q; next }
        $1 == "L" {
            line = substr($0, 3)
            if (match(line, /^- [A-Z][A-Z]-[RE]-[0-9]+/)) {
                id = substr(line, 3, RLENGTH - 2)
                if (id in m) line = substr(line, 1, RLENGTH) " (" m[id] ")" substr(line, RLENGTH + 1)
                if ((id in qt) && match(line, / — user$/)) line = substr(line, 1, RSTART - 1) " — user: \"" qt[id] "\""
            }
            print line
        }'
    echo
}

# spec-delta lines of the cards for the IDs the committed branch diff touches,
# last line per ID, in the shape `sr changes` prints
live_changes() {
    br=$(field branch); [ -n "$br" ] || return 0
    d=$(git -C "$root" diff "main...$br" -- docs/specs 2>/dev/null) || return 0
    touched=$(printf '%s\n' "$d" | grep '^[+-]' | grep -v '^+++\|^---' | grep -o '\*\*[A-Z][A-Z]-[RE]-[0-9]*\*\*' | tr -d '*' | sort -u | tr '\n' ' ')
    [ -n "$touched" ] || return 0
    cat "$board"/*/"$slug".md "$board"/*/"$slug".s*.md 2>/dev/null | awk '$2 == "spec-delta"' | sort -s -k1,1 | awk -v touched=" $touched" '
        index(touched, " " $3 " ") == 0 { next }
        {
            id = $3; kind = $4
            rest = $0; sub(/^[^ ]+ spec-delta [^ ]+ [^ ]+/, "", rest)
            origin = ""
            while (match(rest, /^ +(origin|quote)=/)) {
                field = substr(rest, 1, RLENGTH); sub(/^ +/, "", field)
                rest = substr(rest, RLENGTH + 1)
                if (substr(rest, 1, 1) == "\"") {
                    rest = substr(rest, 2); i = index(rest, "\"")
                    val = substr(rest, 1, i - 1); rest = substr(rest, i + 1)
                } else {
                    i = index(rest, " "); if (i == 0) i = length(rest) + 1
                    val = substr(rest, 1, i - 1); rest = substr(rest, i)
                }
                if (field == "origin=") origin = val
            }
            sub(/^ +/, "", rest)
            n++; if (id in seq) delete byseq[seq[id]]
            seq[id] = n; byseq[n] = "- " id " " kind ": " rest " — " origin
        }
        END { for (i = 1; i <= n; i++) if (i in byseq) print byseq[i] }'
}

# $1: chg_ids
decisions() {
    dec=$({
        [ -f "$a/architecture.md" ] && awk '/^## Decisions$/ { on = 1; next } /^## / { on = 0 } on && /^- /' "$a/architecture.md"
        [ -f "$a/plan.md" ] && awk '/^## Shared$/ { sh = 1; next } /^## / { sh = 0 } sh && /^Decisions:/ { on = 1; next } on && /^- / { print; next } on { on = 0 }' "$a/plan.md"
    } | awk -v ids=" $1" '
        !match($0, / \((chosen|user)(, rev [0-9]+)?\) \[(design|process|spec [A-Z0-9-]+)\]( — .*)?$/) { next }
        {
            choice = substr($0, 3, RSTART - 3); meta = substr($0, RSTART + 2)
            who = meta; sub(/[,)].*/, "", who)
            tag = meta; sub(/^[^[]*\[/, "", tag); sub(/\].*/, "", tag)
            if (tag == "design") r = choice " (" who ")"
            else if (tag ~ /^spec / && who == "chosen") { if (index(ids, " " substr(tag, 6) " ")) next; r = choice " (" who ")" }
            else next
            if (!(r in seen)) { seen[r] = 1; print "- " r }
        }')
    [ -n "$dec" ] && printf '## Decisions\n%s\n\n' "$dec"
}

draft() {
    draft_head
    if [ -n "$scenarios" ]; then
        printf '## Scenarios\nThe %s scenarios of %s: %s\n\n' "${scenarios%%	*}" "$ref" "${scenarios#*	}"
    fi
    echo "## Stages"
    # a stage's verification lines, as sub-items: `  - beyond unit tests: …` and its own sub-lines one level deeper
    verif() {
        awk -v s="$1" '
            /^Verification beyond unit tests:/ { on = 1; next }
            on && match($0, /^- s[0-9]+(, s[0-9]+)* — /) {
                lst = $0; sub(/ — .*/, "", lst); sub(/^- /, "", lst)
                rest = $0; sub(/^- s[0-9]+(, s[0-9]+)* — /, "", rest)
                cur = index(", " lst ",", ", " s ",") > 0
                if (cur) print "  - beyond unit tests: " rest
                next
            }
            on && /^  - / { if (cur) print "  " $0; next }
            on { on = 0 }' "$a/plan.summary.md"
    }
    grep '^| s[0-9]' "$a/plan.summary.md" | while IFS='|' read -r _ sid what _; do
        sid=$(printf '%s' "$sid" | tr -d ' '); what=$(printf '%s' "$what" | sed 's/^ *//; s/ *$//')
        state=; csha=; blocked=
        for d in open inprogress inreview done; do
            c=$board/$d/$slug.$sid.md
            [ -f "$c" ] || continue
            state=$d
            csha=$(awk '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, "commit=") == 1) v = substr($i, 8) } END { print v }' "$c")
            blocked=$(awk '/^blocked-by:/ { gsub(/.*\[|\].*/, ""); print }' "$c")
            break
        done
        ids=$(printf '%s' "$blocked" | tr -d ' ' | tr ',' '\n' | sed 's/.*\.//' | paste -sd, -)
        mark="<!-- $sid${ids:+ blocked-by $ids} -->"
        case $state in
            done) printf -- '- [x] %s (%s) %s\n' "$what" "${csha:-no commit}" "$mark" ;;
            inreview) printf -- '- [ ] %s — in review %s\n' "$what" "$mark" ;;
            inprogress) printf -- '- [ ] %s — implementing %s\n' "$what" "$mark" ;;
            open)
                open_blockers=
                for b in $(printf '%s' "$blocked" | tr -d ' ' | tr ',' ' '); do
                    [ -f "$board/done/$b.md" ] || open_blockers="$open_blockers ${b##*.}"
                done
                if [ -n "$open_blockers" ]; then printf -- '- [ ] %s — blocked %s\n' "$what" "$mark"
                else printf -- '- [ ] %s — next %s\n' "$what" "$mark"; fi ;;
            *) printf -- '- [ ] %s %s\n' "$what" "$mark" ;;
        esac
        verif "$sid"
    done
    echo

    echo "## Review here"
    landed=
    for sid in $(ls "$board/done/" 2>/dev/null | sed -n "s/^$slug\.\(s[0-9]*\)\.md$/\1/p"); do
        landed="$landed${landed:+, }$(what_of "$sid")"
    done
    if [ -n "$landed" ]; then echo "- on the branch: $landed — inline comments on their files are answered before the next stop"
    else echo "- nothing on the branch yet — inline comments are answered once a stage lands"; fi
    if [ "$lane" = fast ]; then echo "- later stages land on green, no stop until gate 4"
    else echo "- later stages land after their review and the user's approval"; fi
    echo

    requirements
    spec_changes "$(live_changes)"
    if [ -f "$a/architecture.summary.md" ]; then architecture; echo; fi
    decisions "$chg_ids"
}

full() {
    [ -f "$a/pr.md" ] || { echo "pr-body: missing $a/pr.md" >&2; exit 1; }
    entries=; [ -f "$a/spec-diff.md" ] && {
        n_new=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — new' "$a/spec-diff.md")
        n_chg=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — changed' "$a/spec-diff.md")
        n_vio=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — violated' "$a/spec-diff.md")
        n_drp=$(grep -c '^## [A-Z][A-Z]-[RE]-[0-9]* — dropped' "$a/spec-diff.md")
        if [ "$n_vio" -gt 0 ]; then entries="$n_vio entries now honoured"
        else entries="$((n_new + n_chg + n_drp)) entries ($n_new new, $n_chg changed${n_drp:+, $n_drp dropped})"; [ "$n_drp" -eq 0 ] && entries="$((n_new + n_chg)) entries ($n_new new, $n_chg changed)"; fi
    }
    cov=$(field log:cov); review=$(field log:review)
    line="$Spec_tok"
    [ "$stages" -gt 0 ] && line="$line · $stages stages"
    [ -n "$entries" ] && line="$line · $entries"
    [ -n "$cov" ] && line="$line · coverage $cov"
    [ -n "$review" ] && line="$line · review $review"
    [ -n "$split" ] && line="$line · split off: $(printf '%s' "$split" | tr -d '[] ' | sed 's/\([0-9][0-9]*\)/#\1/g; s/,/, /g')"
    head -1 "$a/issue.md"
    echo
    printf '%s\n\n' "$line"
    tail -n +2 "$a/pr.md" | sed '1{/^$/d;}'
    echo
    chg_ids=
    if [ -f "$a/spec-diff.md" ] && [ -n "$scenarios" ]; then
        printf '## Scenarios covered\nAll %s scenarios of %s: %s\n\n' "${scenarios%%	*}" "$ref" "${scenarios#*	}"
    fi
    requirements
    if [ -f "$a/spec-diff.md" ]; then spec_changes "$(sr changes | trim)"; fi
    if [ -f "$a/architecture.summary.md" ]; then architecture; echo; fi
    decisions "$chg_ids"
}

if [ "$form" = draft ]; then
    dups=$(ls "$board"/*/"$slug".s*.md 2>/dev/null | xargs -n1 basename | sort | uniq -d | tr '\n' ' ')
    if [ -n "$dups" ]; then
        draft_head
        echo "pr-body: card in more than one state directory: $dups— merge the log lines and delete the stale copy first" >&2
        exit 2
    fi
fi
out=$(mktemp); trap 'rm -f "$out"' EXIT INT TERM
{
    if [ "$form" = draft ]; then draft; else full; fi
    [ -n "$issue" ] && printf 'Closes #%s\n' "$issue"
} | trim > "$out"
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
