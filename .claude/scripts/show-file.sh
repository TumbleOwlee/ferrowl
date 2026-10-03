#!/bin/sh
# Open files for the user without their contents entering the agent's context.
#
#   sh .claude/scripts/show-file.sh <path> [<path> ...]
#
# The files are shown together and listed together in the approval marker.
# Every file is validated first; one refused or unreadable file fails the
# whole call before any marker is written or viewer opened.
#
# Approval marker: for each artifact dir `.claude/tasks/artifacts/<slug>/`
# touched by the call, `<slug>/approval` is overwritten with the absolute
# paths of that dir's files from this call, one per line, in argument order.
# Only direct children of the dir count; nested paths, files outside, and a
# file named `approval` get no entry. Each marker is replaced atomically; with
# several dirs, a failure can leave earlier dirs updated. Editors may watch it;
# this script knows no editor. A marker write failure is fatal (exit 1).
#
# Viewer: SHOW_FILE_VIEWER=none opens nothing; any other value runs as a
# command with the absolute path appended; unset or empty uses the default
# chain: tmux pane with glow, wslview (Windows default app), tmux pane with
# less, else a hint to open the file manually.
#
# Output is one line per file naming how it was shown, never content; the line
# of an artifact file ends with ` (approval marker: <dir>/approval)`.
#
# A user-facing summary (`*.summary.md`, `*.verdict.md`) is capped at
# SUMMARY_MAX_LINES lines and SUMMARY_MAX_BYTES bytes: over the cap the call is
# refused with one line on stderr (exit 3) so the agent that wrote it trims it.
#
# Exit codes: 1 unreadable file, marker write failure, or a viewer failed;
# 2 usage; 3 over the summary cap.

[ $# -ge 1 ] || { echo "usage: $0 <path> [<path> ...]" >&2; exit 2; }

SUMMARY_MAX_LINES=25
SUMMARY_MAX_BYTES=2048
for file in "$@"; do
    [ -r "$file" ] || { echo "show-file: not readable: $file" >&2; exit 1; }
    case "$file" in
        *.summary.md|*.verdict.md)
            lines=$(wc -l < "$file")
            bytes=$(wc -c < "$file")
            if [ "$lines" -gt "$SUMMARY_MAX_LINES" ] || [ "$bytes" -gt "$SUMMARY_MAX_BYTES" ]; then
                echo "show-file: refused: $file is $lines lines / $bytes bytes, cap is $SUMMARY_MAX_LINES lines / $SUMMARY_MAX_BYTES bytes; have the agent that wrote it trim it, never show the full file instead" >&2
                exit 3
            fi ;;
    esac
done

abs_of() {
    d=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd) && printf '%s/%s\n' "$d" "$(basename -- "$1")"
}

marker_dir_of() {
    rest=${1##*/.claude/tasks/artifacts/}
    [ "$rest" = "$1" ] && return 0
    case $rest in
        */*/*|/*|*/|approval|*/approval) ;;
        ?*/?*) printf '%s\n' "${1%/*}" ;;
    esac
}

nl='
'
seen=$nl
drop_temps() {
    set -f
    IFS=$nl
    for d in $seen; do
        [ -n "$d" ] && rm -f "$d/.approval.$$"
    done
    unset IFS
    set +f
}
marker_fail() {
    drop_temps
    echo "show-file: cannot write approval marker: $1/approval" >&2
    exit 1
}

for file in "$@"; do
    abs=$(abs_of "$file") || { echo "show-file: not readable: $file" >&2; exit 1; }
    dir=$(marker_dir_of "$abs")
    [ -n "$dir" ] || continue
    case "$seen" in
        *"$nl$dir$nl"*) printf '%s\n' "$abs" >> "$dir/.approval.$$" || marker_fail "$dir" ;;
        *)
            seen=$seen$dir$nl
            printf '%s\n' "$abs" > "$dir/.approval.$$" || marker_fail "$dir" ;;
    esac
done
set -f
IFS=$nl
for d in $seen; do
    [ -n "$d" ] || continue
    mv -f "$d/.approval.$$" "$d/approval" || { unset IFS; set +f; marker_fail "$d"; }
done
unset IFS
set +f

has() { command -v "$1" >/dev/null 2>&1; }

rc=0
for file in "$@"; do
    abs=$(abs_of "$file")
    dir=$(marker_dir_of "$abs")
    suffix=
    [ -n "$dir" ] && suffix=" (approval marker: $dir/approval)"
    if [ "$SHOW_FILE_VIEWER" = none ]; then
        echo "show-file: no viewer (SHOW_FILE_VIEWER=none): $abs$suffix"
    elif [ -n "$SHOW_FILE_VIEWER" ]; then
        # Unquoted on purpose: "code -r" splits into command and arguments.
        $SHOW_FILE_VIEWER "$abs" && echo "show-file: opened with $SHOW_FILE_VIEWER: $abs$suffix" || rc=1
    elif [ -n "$TMUX" ] && has glow; then
        tmux split-window -v "glow -p '$abs'" && echo "show-file: opened in tmux pane (glow): $abs$suffix" || rc=1
    elif has wslview; then
        wslview "$abs" && echo "show-file: opened with wslview: $abs$suffix" || rc=1
    elif [ -n "$TMUX" ]; then
        tmux split-window -v "less '$abs'" && echo "show-file: opened in tmux pane (less): $abs$suffix" || rc=1
    else
        echo "show-file: no viewer available (need tmux+glow, wslview, or tmux+less); open manually: $abs$suffix"
    fi
done
exit $rc
