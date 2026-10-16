#!/bin/sh
# Entries of docs/specs/*/requirements.md and edge-cases.md that mention a
# keyword (case-insensitive substring), so a new entry cites or reconciles with them.
#
#   sh .claude/scripts/related-ids.sh <keyword> [<keyword> ...]
#
# One line per hit: `<ID> — <area>/<file> `## <heading>` — <text>`, text cut
# to 100 characters. Entries are read in all three forms the specs use:
# `**ID** — text`, `- **ID** — text` and the table row `| **ID** | cell | … |`
# (cells joined by ` — `). `related=none` and exit 1 without a hit; exit 2
# without a keyword.
[ $# -ge 1 ] || { echo "usage: $0 <keyword> [<keyword> ...]" >&2; exit 2; }
specs=${CLAUDE_PROJECT_DIR:-.}/docs/specs
kw=$(printf '%s\n' "$@" | tr 'A-Z' 'a-z' | paste -sd'\t' -)
hits=$(for kind in requirements edge-cases; do
    for f in "$specs"/*/$kind.md; do
        [ -f "$f" ] || continue
        area=$(basename "$(dirname "$f")")
        awk -v kw="$kw" -v where="$area/$kind.md" '
            BEGIN { nk = split(kw, k, "\t") }
            /^## / { h = $0; sub(/^## /, "", h); next }
            {
                id = ""
                if (match($0, /^(- )?\*\*[A-Z][A-Z]-[RE]-[0-9]+\*\* — /)) {
                    id = substr($0, RSTART, RLENGTH); text = substr($0, RSTART + RLENGTH)
                } else if (match($0, /^\| \*\*[A-Z][A-Z]-[RE]-[0-9]+\*\* \|/)) {
                    id = substr($0, RSTART, RLENGTH); text = substr($0, RSTART + RLENGTH)
                    sub(/ *\|[ ]*$/, "", text)
                    n = split(text, c, /\|/); text = ""
                    for (i = 1; i <= n; i++) { gsub(/^[ \t]+|[ \t]+$/, "", c[i]); if (c[i] != "") text = text (text == "" ? "" : " — ") c[i] }
                }
                if (id == "") next
                match(id, /[A-Z][A-Z]-[RE]-[0-9]+/); id = substr(id, RSTART, RLENGTH)
                low = tolower(text); hit = 0
                for (i = 1; i <= nk; i++) if (index(low, k[i])) { hit = 1; break }
                if (!hit) next
                if (length(text) > 100) text = substr(text, 1, 99) "…"
                print id " — " where (h == "" ? "" : " `## " h "`") " — " text
            }' "$f"
    done
done)
[ -n "$hits" ] || { echo "related=none"; exit 1; }
printf '%s\n' "$hits"
