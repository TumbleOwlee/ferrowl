#!/bin/sh
# PreToolUse guard on the Bash tool for the spec-reviewer subagent, which is
# read-only by contract: it reports, it never fixes. The prose rule alone was
# not enough — a reviewer once ran `git checkout <file>` inside an
# implementer's worktree to probe something and wiped an uncommitted stage
# diff. This hook makes the rule mechanical: every command segment (split on
# `;`, `&&`, `||`, `|`, `&`, newline) is checked and the whole call is denied
# if any segment could alter the repo, the working tree, or the filesystem —
# mutating git subcommands, file-changing coreutils, in-place editors,
# interpreters (a script can write anything), mutating package-manager/gh
# subcommands, and any output redirection. The `cargo` case below is the one
# stack-specific block: extend it with this project's package manager's
# mutating subcommands (npm/pnpm install|add|remove, pip/uv add|sync, go get,
# …); an unmatched command name never fires, so a foreign block is harmless. The sanctioned writes are an append (`>>` or
# `tee -a`) to `.claude/tasks/artifacts/<slug>/review.md` and a rewrite (`>`
# or `tee`) of `.claude/tasks/artifacts/<slug>/review.verdict.md`; everything
# else the reviewer wanted to try belongs in the review as a finding for the
# implementer.
#
# Reads a PreToolUse hook payload on stdin, writes a deny-decision JSON
# object on stdout when it blocks, nothing when it doesn't.
set -eu
set -f

input=$(cat)
name=$(printf '%s' "$input" | jq -r '.tool_name // empty')
[ "$name" = "Bash" ] || exit 0

cwd=$(printf '%s' "$input" | jq -r '.cwd // "."')
cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
[ -n "$cmd" ] || exit 0

# Drop heredoc bodies: a review finding quoting `git checkout` is text, not a
# command. The `<<` line itself stays so its redirect is still checked.
strip_heredocs=$(cat <<'AWK'
skip { if ($0 == term || $0 ~ ("^\t*" term "$")) skip = 0; next }
{
  print
  if (match($0, /<<-?[ \t]*['"]?[A-Za-z_][A-Za-z0-9_]*/)) {
    t = substr($0, RSTART, RLENGTH)
    sub(/^<<-?[ \t]*['"]?/, "", t)
    term = t
    skip = 1
  }
}
AWK
)

# Then drop fd duplications (`2>&1`, `>&2`) and quoted strings so a grep for
# `=>` is not mistaken for a redirect, and split into segments.
segments=$(printf '%s\n' "$cmd" \
  | awk "$strip_heredocs" \
  | sed -e 's/[0-9]*>&[0-9]//g' -e "s/'[^']*'//g" -e 's/"[^"]*"//g' \
  | tr ';|&' '\n\n\n')

offender=""

is_verdict_file() {
  t=$1
  case "$t" in /*) ;; *) t="$cwd/$t" ;; esac
  case "$t" in
    */.claude/tasks/artifacts/*/review.verdict.md) return 0 ;;
  esac
  return 1
}

is_review_append() {
  t=$1
  case "$t" in /*) ;; *) t="$cwd/$t" ;; esac
  case "$t" in
    */.claude/tasks/artifacts/*/review.md) return 0 ;;
  esac
  is_verdict_file "$t"
}

# Sets `sub` to the first non-option word after the command name, skipping
# the argument of `-C`/`-c`, and `args` to what follows it.
subcommand() {
  sub=""; args=""; skipnext=0
  for t in "$@"; do
    if [ "$skipnext" = 1 ]; then skipnext=0; continue; fi
    case "$t" in
      -C|-c) skipnext=1 ;;
      -*|+*) ;;
      *) if [ -z "$sub" ]; then sub=$t; else args="$args $t"; fi ;;
    esac
  done
}

old_ifs=$IFS
IFS='
'
for seg in $segments; do
  IFS=$old_ifs
  set -- $seg
  [ $# -gt 0 ] || { IFS='
'; continue; }

  # Command word: past leading `(`/`{`/`!`, env assignments and wrappers.
  while [ $# -gt 0 ]; do
    case "$1" in
      '('|'{'|'!'|*=*|sudo|env|command|time|nice|nohup|exec|xargs) shift ;;
      '('*|'{'*|'!'*) w=$1; shift; set -- "${w#?}" "$@" ;;
      -*) shift ;;
      *) break ;;
    esac
  done
  [ $# -gt 0 ] || { IFS='
'; continue; }
  name=${1##*/}
  shift

  case "$name" in
    rm|mv|cp|ln|mkdir|touch|chmod|chown|truncate|dd|install|rmdir|shred|mkfifo|mknod|eval)
      offender=$seg ;;
    python|python2|python3|node|ruby|perl|php)
      offender=$seg ;;
    sh|bash|dash|zsh|awk|gawk|mawk)
      case " $* " in
        *' -c '*|*' -e '*|*'system('*) offender=$seg ;;
      esac
      case "$name" in awk|gawk|mawk) case "$seg" in *'>'*) offender=$seg ;; esac ;; esac ;;
    sed)
      for t in "$@"; do
        case "$t" in --in-place*|-i*|-[!-]*i*) offender=$seg; break ;; esac
      done ;;
    find)
      case " $* " in
        *' -delete '*|*' -exec '*|*' -execdir '*|*' -ok '*|*' -okdir '*|*' -fprint'*) offender=$seg ;;
      esac ;;
    tee)
      append=0
      for t in "$@"; do
        case "$t" in
          -a|--append) append=1 ;;
          -*) ;;
          *) if [ "$append" = 1 ]; then is_review_append "$t" || offender=$seg
             else is_verdict_file "$t" || offender=$seg; fi ;;
        esac
      done ;;
    git)
      subcommand "$@"
      case "$sub" in
        checkout|switch|restore|reset|clean|commit|add|rm|mv|merge|rebase|cherry-pick|revert|apply|am|push|pull|notes|update-ref|symbolic-ref|filter-branch|gc|prune|reflog|submodule|remote)
          offender=$seg ;;
        stash)   case "$args" in ' list'*|' show'*) ;; *) offender=$seg ;; esac ;;
        worktree) case "$args" in ' list'*) ;; *) offender=$seg ;; esac ;;
        fetch)   case " $* " in *' --prune '*|*' -p '*|*' -P '*) offender=$seg ;; esac ;;
        branch)  case " $* " in *' -d '*|*' -D '*|*' -m '*|*' -M '*|*' --delete '*|*' --move '*|*' -f '*|*' --force '*|*' -u '*|*' --set-upstream-to'*|*' --unset-upstream '*) offender=$seg ;; esac ;;
        tag)     case "$args" in '') case " $* " in *' -l '*|*' --list '*|*' -n'*|' tag '|*' --contains '*|*' --points-at '*) ;; *) offender=$seg ;; esac ;; *) offender=$seg ;; esac ;;
        config)  case " $* " in *' --get'*|*' -l '*|*' --list '*|*' get '*|*' list '*) ;; *) offender=$seg ;; esac ;;
      esac ;;
    cargo)  # stack-specific: see header
      subcommand "$@"
      case "$sub" in
        fix|install|uninstall|add|remove|rm|update|publish|init|new|generate-lockfile|vendor|yank|login|logout) offender=$seg ;;
        fmt)    case " $* " in *' --check '*) ;; *) offender=$seg ;; esac ;;
        clippy) case " $* " in *' --fix '*) offender=$seg ;; esac ;;
      esac ;;
    gh)
      subcommand "$@"
      case "$sub" in
        api) case " $* " in *' -X '*|*' --method '*|*' -f '*|*' -F '*|*' --field '*|*' --raw-field '*|*' --input '*) offender=$seg ;; esac ;;
        auth|config|alias|extension|secret|variable|ssh-key|gpg-key) offender=$seg ;;
        *)
          for t in $args; do
            case "$t" in
              create|comment|edit|close|reopen|merge|review|delete|ready|lock|unlock|pin|unpin|transfer|checkout|develop|clone|fork|sync|rename|archive|unarchive|set-default|rerun|cancel|run|download|upload) offender=$seg ;;
            esac
            break
          done ;;
      esac ;;
  esac

  # Output redirection: an append onto review.md, or a rewrite of review.verdict.md.
  if [ -z "$offender" ]; then
    want=""
    for t in $seg; do
      if [ -n "$want" ]; then
        case "$want" in
          '>>') is_review_append "$t" || offender=$seg ;;
          *)    [ "$t" = /dev/null ] || is_verdict_file "$t" || offender=$seg ;;
        esac
        want=""
        continue
      fi
      case "$t" in
        *'>>'*) rest=${t#*>>}; if [ -z "$rest" ]; then want='>>'; else is_review_append "$rest" || offender=$seg; fi ;;
        *'>'*)  rest=${t#*>};  if [ -z "$rest" ]; then want='>';  else [ "$rest" = /dev/null ] || is_verdict_file "$rest" || offender=$seg; fi ;;
      esac
      [ -z "$offender" ] || break
    done
    [ -z "$want" ] || offender=$seg
  fi

  IFS='
'
  [ -z "$offender" ] || break
done
IFS=$old_ifs

if [ -n "$offender" ]; then
  reason="spec-reviewer is read-only (hook-enforced): '$offender' would alter the repo, the working tree, or the filesystem. The only sanctioned writes are an append ('>>' or 'tee -a') onto .claude/tasks/artifacts/<slug>/review.md and a rewrite ('>' or 'tee') of .claude/tasks/artifacts/<slug>/review.verdict.md. Never run git checkout/stash/reset or anything that touches the worktree — record what you needed to probe as a finding for the implementer instead."
  jq -n --arg reason "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $reason}}'
fi
exit 0
