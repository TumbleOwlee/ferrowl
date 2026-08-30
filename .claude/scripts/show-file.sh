#!/bin/sh
# Open a file for the user without its contents entering the agent's context.
# Preference order: tmux pane with glow, wslview (Windows default app), tmux
# pane with less, else print a hint to open the file manually. Prints one
# line naming the viewer used; never the file.
#
#   sh .claude/scripts/show-file.sh <path>

[ $# -eq 1 ] || { echo "usage: $0 <path>" >&2; exit 2; }
file=$1
[ -r "$file" ] || { echo "show-file: not readable: $file" >&2; exit 1; }

case "$file" in
    /*) abs=$file ;;
    *)  abs="$(pwd)/$file" ;;
esac

has() { command -v "$1" >/dev/null 2>&1; }

if [ -n "$TMUX" ] && has glow; then
    tmux split-window -h "glow -p '$abs'" && echo "show-file: opened in tmux pane (glow): $abs"
elif has wslview; then
    wslview "$abs" && echo "show-file: opened with wslview: $abs"
elif [ -n "$TMUX" ]; then
    tmux split-window -h "less '$abs'" && echo "show-file: opened in tmux pane (less): $abs"
else
    echo "show-file: no viewer available (need tmux+glow, wslview, or tmux+less); open manually: $abs"
fi
