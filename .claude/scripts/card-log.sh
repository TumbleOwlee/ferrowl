#!/bin/sh
# Appends one validated line to a task card, wherever the board moved it.
#   card-log.sh <card-id> <event> [<positional>...] [k=v ...] [-- <free text>]
# Prints nothing on success. Exit 1: card not found or ambiguous
# (`card-log=card`); exit 2: usage or a line the vocabulary rejects
# (`card-log=reject event=<e> reason=<event|key|scope|value|length|budget>`;
# budget: a fourth `read` on one stage card, READ_BUDGET overrides 3).
# Board: $CLAUDE_PROJECT_DIR/.claude/tasks, else the main checkout's, which
# is also found from inside a worktree. CARD_LOG_NOW overrides the stamp.
#
# Vocabulary, one line per event:
#   <scope: stage|parent|both> <name> <value set: - | any | a|b> <positional: - | id,kind | ids> <keys: - | k1,k2> <text: no|yes|required>
# vocab-begin
# both   spawn          -                 -        agent,stage           no
# stage  test-red       -                 [ids]    -                     yes
# stage  green          -                 -        -                     no
# both   spec-delta     -                 id,kind  origin,quote          yes
# stage  checks=        pass|fail         -        crates                yes
# stage  commit=        any               -        -                     no
# both   feedback=      any               -        -                     no
# stage  stopped        -                 -        -                     required
# stage  follow-up      -                 -        -                     required
# stage  read           -                 -        path                  required
# both   note           -                 -        -                     required
# both   moved=         open|inprogress|inreview|done -  -              no
# stage  minor=         any               -        -                     yes
# stage  minor-moved=   any               -        -                     yes
# stage  files+=        any               -        user,reason           no
# stage  auto=          any               -        -                     no
# stage  stop=          any               -        -                     no
# parent stop           -                 -        kind,gate,agent,count no
# parent spec-rev=      any               -        -                     no
# parent arch-rev=      any               -        -                     no
# parent spec-keep=     any               -        -                     no
# parent pr=            draft|ready       -        -                     no
# parent gauntlet=      pass|fail         -        cov,sha,step,reason   no
# parent replan         -                 -        from                  no
# parent replan=        approved          -        -                     no
# parent plan-review=   any               -        count,round           no
# parent review=        any               -        scope                 no
# parent overturned     -                 -        scope,count           yes
# parent rebuild        -                 -        reason                no
# parent follow-ups=    any               -        -                     no
# parent ci=            any               -        -                     no
# vocab-end
set -u

[ $# -ge 2 ] || { echo "usage: card-log.sh <card-id> <event> [<positional>...] [k=v ...] [-- <text>]" >&2; exit 2; }
id=$1 ev=$2; shift 2

if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then board=$CLAUDE_PROJECT_DIR/.claude/tasks
else
    common=$(git rev-parse --git-common-dir 2>/dev/null) || { echo "card-log=card"; exit 1; }
    board=$(cd "$common/.." && pwd)/.claude/tasks
fi
found=$(ls "$board"/*/"$id".md 2>/dev/null)
case $found in ''|*'
'*) echo "card-log=card"; exit 1 ;; esac
card=$found
case $id in *.s[0-9]*) scope=stage ;; *) scope=parent ;; esac

case $ev in *=*) name=${ev%%=*}= val=${ev#*=} ;; *) name=$ev val= ;; esac
rej() { echo "card-log=reject event=${name%=} reason=$1"; exit 2; }

row=$(sed -n '/^# vocab-begin/,/^# vocab-end/p' "$0" | awk -v n="$name" '$3 == n { print; exit }')
[ -n "$row" ] || rej event
rscope=$(echo "$row" | awk '{print $2}')
vset=$(echo "$row" | awk '{print $4}')
pspec=$(echo "$row" | awk '{print $5}')
keys=$(echo "$row" | awk '{print $6}')
tspec=$(echo "$row" | awk '{print $7}')
[ "$rscope" = both ] || [ "$rscope" = "$scope" ] || rej scope

case $name in *=) 
    [ -n "$val" ] || rej value
    case $val in *'"'*) rej value ;; esac
    [ "$vset" = any ] || case "|$vset|" in *"|$val|"*) ;; *) rej value ;; esac ;;
esac

idre='[A-Z][A-Z]-[RE]-[0-9]+'
npos=0 pos="" kvs="" text="" intext=0 origin=""
for a in "$@"; do
    if [ "$intext" = 1 ]; then text=${text:+$text }$a; continue; fi
    case $a in
        --) intext=1 ;;
        *=*)
            k=${a%%=*} v=${a#*=}
            case ",$keys," in *",$k,"*) ;; *) rej key ;; esac
            [ -n "$v" ] || rej value
            case $v in *'"'*) rej value ;; esac
            case $name$k in
                stopkind) case $v in question|approval|stage|blocked) ;; *) rej value ;; esac ;;
                spec-deltaorigin) case $v in user|review|impl) origin=$v ;; *) rej value ;; esac ;;
            esac
            case $v in *' '*) v="\"$v\"" ;; esac
            kvs="$kvs $k=$v" ;;
        *)
            npos=$((npos + 1))
            case $pspec in
                id,kind)
                    if [ $npos = 1 ]; then echo "$a" | grep -qE "^$idre\$" || rej value
                    elif [ $npos = 2 ]; then case $a in added|changed|dropped) ;; *) rej value ;; esac
                    else rej value; fi ;;
                '[ids]'|ids)
                    [ $npos = 1 ] && echo "$a" | grep -qE "^$idre(,$idre)*\$" || rej value ;;
                *) rej value ;;
            esac
            pos="$pos $a" ;;
    esac
done
[ "$pspec" != id,kind ] || [ $npos = 2 ] || rej value
[ "$name" != stop ] || case $kvs in *' kind='*) ;; *) rej key ;; esac
[ "$name" != spec-delta ] || [ -n "$origin" ] || rej value
[ "$name" != read ] || [ "$(grep -c '^[^ ]* read path=' "$card")" -lt "${READ_BUDGET:-3}" ] || rej budget
case $tspec in
    no) [ "$intext" = 0 ] || rej value ;;
    required) [ -n "$text" ] || rej value ;;
esac

case $val in *' '*) ev="$name\"$val\"" ;; esac
line="${CARD_LOG_NOW:-$(date -u +%FT%H:%M)} $ev$pos$kvs${text:+ $text}"
[ "$(printf '%s' "$line" | wc -l)" -eq 0 ] || rej value
[ "${#line}" -le 160 ] || rej length
printf '%s\n' "$line" >> "$card"
