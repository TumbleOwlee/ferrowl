#!/bin/sh
# Move a task card to another state directory, refusing a transition the
# board's rules forbid, and log the move on the card.
#
#   sh .claude/scripts/card-move.sh <card-id> <open|inprogress|inreview|done>
#
# Stage card (`<slug>.s<n>`):
#   open → inprogress        every `blocked-by` card is in done/
#   inprogress → inreview    green
#   inreview → inprogress    findings, a red gauntlet
#   inreview → done          the card's last `commit=` sha is the sha of a
#                            `gauntlet=pass` line on the parent card (pushed and verified)
#   done → inprogress        a fix after done: gate 3 findings, PR feedback, red CI
# Parent card (`<slug>`):
#   open → inprogress        `gate2:` is set
#   open → inreview          `kind: epic` (an epic has no implementation)
#   inprogress → inreview    gate 3 starts
#   inreview → inprogress    a fix after gate 3
#   never done — the run ends in run-cleanup.sh
# Moving a card to the state it is in is a no-op. Prints nothing on success.
# Exit 1: card not found or ambiguous (`card-move=card`); exit 2: usage or a
# refused move (`card-move=refused from=<s> to=<s> reason=<transition|blocked-by|commit|unverified|gate2>`).
set -u
[ $# -eq 2 ] || { echo "usage: $0 <card-id> <open|inprogress|inreview|done>" >&2; exit 2; }
id=$1 to=$2
case $to in open|inprogress|inreview|done) ;; *) echo "usage: $0 <card-id> <open|inprogress|inreview|done>" >&2; exit 2 ;; esac

if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then board=$CLAUDE_PROJECT_DIR/.claude/tasks
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "card-move=card"; exit 1; }
    board=$(cd "$common/.." && pwd)/.claude/tasks
fi
card=$(ls "$board"/open/"$id".md "$board"/inprogress/"$id".md "$board"/inreview/"$id".md "$board"/done/"$id".md 2>/dev/null)
case $card in ''|*'
'*) echo "card-move=card"; exit 1 ;; esac
from=$(basename "$(dirname "$card")")
[ "$from" = "$to" ] && exit 0
refuse() { echo "card-move=refused from=$from to=$to reason=$1"; exit 2; }

fm() { awk -v k="$2" '/^---$/ { if (fm) exit; fm = 1; next } fm && index($0, k ":") == 1 { sub("^" k ":[ ]*", ""); print; exit }' "$1"; }
case $id in
    *.s[0-9]*)
        slug=${id%.s*}
        case $from-$to in
            open-inprogress)
                for b in $(fm "$card" blocked-by | tr -d '[]' | tr ',' ' '); do
                    case $b in *.*) ;; *) b=$slug.$b ;; esac
                    [ -f "$board/done/$b.md" ] || refuse blocked-by
                done ;;
            inprogress-inreview|inreview-inprogress|done-inprogress) ;;
            inreview-done)
                sha=$(awk '/^---$/ { fm = !fm; next } !fm { for (i = 2; i <= NF; i++) if (index($i, "commit=") == 1) v = substr($i, 8) } END { print v }' "$card")
                [ -n "$sha" ] || refuse commit
                parent=$(ls "$board"/*/"$slug".md 2>/dev/null | head -1)
                [ -n "$parent" ] && awk -v s="$sha" '/^---$/ { fm = !fm; next }
                    !fm && / gauntlet=pass/ { for (i = 2; i <= NF; i++) if (index($i, "sha=") == 1) { p = substr($i, 5); if (index(p, s) == 1 || index(s, p) == 1) f = 1 } }
                    END { exit !f }' "$parent" || refuse unverified ;;
            *) refuse transition ;;
        esac ;;
    *)
        case $from-$to in
            open-inprogress) [ -n "$(fm "$card" gate2)" ] || refuse gate2 ;;
            open-inreview) [ "$(fm "$card" kind)" = epic ] || refuse transition ;;
            inprogress-inreview|inreview-inprogress) ;;
            *) refuse transition ;;
        esac ;;
esac
mv "$card" "$board/$to/$id.md" || { echo "card-move=card"; exit 1; }
sh "$(dirname -- "$0")/card-log.sh" "$id" "moved=$to" >/dev/null
