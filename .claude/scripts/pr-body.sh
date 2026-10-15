#!/bin/sh
# Assemble the PR body from a run's artifacts and cards.
#
#   sh .claude/scripts/pr-body.sh <slug> draft > <out>    # from the first push to gate 4; rerun on every card move
#   sh .claude/scripts/pr-body.sh <slug> full  > <out>    # at gate 4, from spec-author's pr.md
#
# Line 1 is `# <title>` (stripped for --title). Both forms end with
# `Closes #<issue>` when the parent card carries `issue`.
#
# draft:  status line (lane, mode, spec rev, last green sha from the parent
#         card's log), ## Why (first line of issue.md's ## Goal), ## Stages
#         (one checklist line per stage card: ticked with its sha in done/,
#         else its board state), ## Review here.
# full:   status line (spec rev, stages, entries, coverage, review, splits),
#         then pr.md from line 2 on.
[ $# -eq 2 ] || { echo "usage: $0 <slug> draft|full" >&2; exit 2; }
slug=$1; form=$2
root=${CLAUDE_PROJECT_DIR:-.}
a=$root/.claude/tasks/artifacts/$slug
board=$root/.claude/tasks
field() { sh "$root/.claude/scripts/card-field.sh" "$slug" "$1"; }
[ -f "$a/issue.md" ] || { echo "pr-body: missing $a/issue.md" >&2; exit 1; }

issue=$(field issue); lane=$(field lane); mode=$(field mode); split=$(field split)
rev=; [ -f "$a/spec-diff.md" ] && rev=$(sed -n '1s/.*(rev \([0-9]*\)).*/\1/p' "$a/spec-diff.md")
spec_tok="no spec change"; [ -n "$rev" ] && spec_tok="spec rev $rev"
Spec_tok="No spec change"; [ -n "$rev" ] && Spec_tok="Spec rev $rev"
stages=0; [ -f "$a/plan.summary.md" ] && stages=$(grep -c '^| s[0-9]' "$a/plan.summary.md")

head -1 "$a/issue.md"
echo

case $form in
draft)
    sha=$(field pass-sha)
    line="Draft"
    [ -n "$lane" ] && line="$line · lane $lane"
    [ -n "$mode" ] && line="$line · $mode"
    line="$line · $spec_tok"
    [ -n "$sha" ] && line="$line · gauntlet green at $sha"
    printf '%s\n\n' "$line"

    echo "## Why"
    sed -n '/^## Goal$/{ n; p; q; }' "$a/issue.md"
    echo

    dups=$(ls "$board"/*/"$slug".s*.md 2>/dev/null | xargs -n1 basename | sort | uniq -d | tr '\n' ' ')
    if [ -n "$dups" ]; then
        echo "pr-body: card in more than one state directory: $dups— merge the log lines and delete the stale copy first" >&2
        exit 2
    fi
    echo "## Stages"
    last_done=; next_line=
    grep '^| s[0-9]' "$a/plan.summary.md" | while IFS='|' read -r _ sid what _; do
        sid=$(printf '%s' "$sid" | tr -d ' '); what=$(printf '%s' "$what" | sed 's/^ *//; s/ *$//')
        state=; csha=
        for d in open inprogress inreview done; do
            c=$board/$d/$slug.$sid.md
            [ -f "$c" ] || continue
            state=$d
            csha=$(awk '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, "commit=") == 1) v = substr($i, 8) } END { print v }' "$c")
            blocked=$(awk '/^blocked-by:/ { gsub(/.*\[|\].*/, ""); print }' "$c")
            break
        done
        case $state in
            done) printf -- '- [x] %s — %s (%s)\n' "$sid" "$what" "${csha:-no commit}" ;;
            inreview) printf -- '- [ ] %s — %s — in review\n' "$sid" "$what" ;;
            inprogress) printf -- '- [ ] %s — %s — implementing\n' "$sid" "$what" ;;
            open)
                open_blockers=
                for b in $(printf '%s' "$blocked" | tr ',' ' '); do
                    [ -f "$board/done/$b.md" ] || open_blockers="$open_blockers ${b##*.}"
                done
                if [ -n "$open_blockers" ]; then printf -- '- [ ] %s — %s — blocked by%s\n' "$sid" "$what" "$open_blockers"
                else printf -- '- [ ] %s — %s — next\n' "$sid" "$what"; fi ;;
            *) printf -- '- [ ] %s — %s\n' "$sid" "$what" ;;
        esac
    done
    echo

    echo "## Review here"
    landed=$(ls "$board/done/" 2>/dev/null | sed -n "s/^$slug\.\(s[0-9]*\)\.md$/\1/p" | tr '\n' ' ' | sed 's/ $//')
    if [ -n "$landed" ]; then echo "- on the branch: $landed — inline comments on their files come back to the implementer before the next stop"
    else echo "- nothing on the branch yet — inline comments come back to the implementer once a stage lands"; fi
    if [ "$lane" = fast ]; then echo "- later stages land on green, no stop until gate 4"
    else echo "- later stages land after their review and the user's approval"; fi
    ;;
full)
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
    printf '%s\n\n' "$line"
    tail -n +2 "$a/pr.md" | sed '1{/^$/d;}'
    ;;
*) echo "usage: $0 <slug> draft|full" >&2; exit 2 ;;
esac

[ -n "$issue" ] && printf '\nCloses #%s\n' "$issue"
exit 0
