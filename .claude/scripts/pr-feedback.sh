#!/usr/bin/env bash
# Bridge between the draft PR (the user's review surface) and the workflow.
# Backend auto-detected like pr-view.sh: GitHub (.github/workflows/*.yml,
# via `gh`) or Bitbucket (bitbucket-pipelines.yml, via REST using
# .claude/bitbucket.local.json).
#
#   pr-feedback.sh fetch <number> <out-file>
#     Writes every unresolved inline review thread of the PR to <out-file>
#     (path:line, author, comment bodies, an empty `reply:` line per thread)
#     and prints ONE status line:
#       pr=<n> state=<open|merged|closed|declined> draft=<true|false> threads=<count> file=<out-file>
#     The orchestrator holds only that line; the implementer reads the file.
#
#   pr-feedback.sh reply <number> <file>
#     For every thread in <file> whose `reply:` line is non-empty, posts that
#     text as a reply on the thread and marks the thread resolved. Threads
#     with an empty `reply:` line are left untouched. Prints
#       pr=<n> replied=<count> skipped=<count>
#
# File shape written by `fetch` (agents fill `reply:` only, never the rest):
#
#   # PR feedback — fetched 2026-01-02T14:02
#   ## thread <thread-id>
#   comment: <first-comment-id>
#   path: src/x.ext:42
#   [alice @ 2026-01-02T13:50]
#   <comment body, verbatim>
#   [bob @ 2026-01-02T13:55]
#   <comment body, verbatim>
#   reply:
set -euo pipefail

usage() {
  echo "Usage: pr-feedback.sh fetch <number> <out-file> | reply <number> <file>" >&2
  exit 1
}

[ $# -eq 3 ] || usage
mode="$1"; pr="$2"; file="$3"
case "$mode" in fetch|reply) ;; *) usage ;; esac

if ls .github/workflows/*.y*ml >/dev/null 2>&1; then
  host=github
elif ls bitbucket-pipelines.y*ml >/dev/null 2>&1; then
  host=bitbucket
  creds=.claude/bitbucket.local.json
  if [ ! -f "$creds" ]; then
    echo "bitbucket-pipelines.yml found but $creds is missing — no credentials to call the Bitbucket API" >&2
    exit 1
  fi
  workspace=$(jq -r .workspace "$creds")
  repo=$(jq -r .repoSlug "$creds")
  user=$(jq -r .username "$creds")
  pass=$(jq -r .appPassword "$creds")
  api="https://api.bitbucket.org/2.0/repositories/$workspace/$repo/pullrequests/$pr"
else
  echo "No .github/workflows/ or bitbucket-pipelines.yml found — can't detect PR host" >&2
  exit 1
fi

# ---------------------------------------------------------------- fetch
fetch_github() {
  gh api graphql -F owner='{owner}' -F repo='{repo}' -F pr="$pr" -f query='
    query($owner: String!, $repo: String!, $pr: Int!) {
      repository(owner: $owner, name: $repo) {
        pullRequest(number: $pr) {
          state isDraft
          reviewThreads(first: 100) {
            nodes {
              id isResolved path line originalLine
              comments(first: 50) { nodes { databaseId author { login } createdAt body } }
            }
          }
        }
      }
    }' | jq -r --arg ts "$(date -u +%FT%H:%M)" '
    .data.repository.pullRequest as $p
    | [$p.reviewThreads.nodes[] | select(.isResolved | not)] as $t
    | "pr=\(env.PR) state=\($p.state | ascii_downcase) draft=\($p.isDraft) threads=\($t | length)",
      "# PR feedback — fetched \($ts)",
      ($t[] |
        "## thread \(.id)",
        "comment: \(.comments.nodes[0].databaseId)",
        "path: \(.path):\(.line // .originalLine // "?")",
        (.comments.nodes[] | "[\(.author.login // "unknown") @ \(.createdAt)]", (.body // "(empty)")),
        "reply:",
        "")'
}

fetch_bitbucket() {
  pr_json=$(curl -sf -u "$user:$pass" "$api")
  comments_json=$(curl -sf -u "$user:$pass" -G "$api/comments" --data-urlencode "pagelen=100")
  jq -rn --arg ts "$(date -u +%FT%H:%M)" --argjson pr "$pr_json" --argjson c "$comments_json" '
    [$c.values[] | select(.inline != null and (.deleted | not) and .parent == null and .resolution == null)] as $roots
    | "pr=\(env.PR) state=\($pr.state | ascii_downcase) draft=\($pr.draft // false) threads=\($roots | length)",
      "# PR feedback — fetched \($ts)",
      ($roots[] | . as $r |
        "## thread \($r.id)",
        "comment: \($r.id)",
        "path: \($r.inline.path):\($r.inline.to // $r.inline.from // "?")",
        "[\($r.user.display_name // "unknown") @ \($r.created_on)]", ($r.content.raw // "(empty)"),
        ($c.values[] | select(.parent.id == $r.id and (.deleted | not))
          | "[\(.user.display_name // "unknown") @ \(.created_on)]", (.content.raw // "(empty)")),
        "reply:",
        "")'
}

# ---------------------------------------------------------------- reply
# Emits "thread-id<TAB>comment-id<TAB>reply text" per thread with a reply.
pending_replies() {
  awk '
    /^## thread /  { tid=$3; cid="" }
    /^comment: /   { cid=$2 }
    /^reply: ./    { sub(/^reply: */, ""); print tid "\t" cid "\t" $0 }
  ' "$file"
}

reply_github() {
  local tid="$1" cid="$2" text="$3"
  gh api "repos/{owner}/{repo}/pulls/$pr/comments/$cid/replies" -f body="$text" >/dev/null
  gh api graphql -F id="$tid" -f query='
    mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }' >/dev/null
}

reply_bitbucket() {
  local tid="$1" cid="$2" text="$3"
  jq -n --arg t "$text" --argjson p "$cid" '{content: {raw: $t}, parent: {id: $p}}' \
    | curl -sf -u "$user:$pass" -H 'Content-Type: application/json' -d @- "$api/comments" >/dev/null
  curl -sf -u "$user:$pass" -X POST "$api/comments/$cid/resolve" >/dev/null
}

export PR="$pr"
case "$mode" in
  fetch)
    out=$("fetch_$host")
    printf '%s\n' "$out" | tail -n +2 > "$file"
    printf '%s file=%s\n' "$(printf '%s\n' "$out" | head -n1)" "$file"
    ;;
  reply)
    [ -f "$file" ] || { echo "No such file: $file" >&2; exit 1; }
    replied=0; skipped=$(grep -c '^## thread ' "$file" || true)
    while IFS=$'\t' read -r tid cid text; do
      [ -n "$tid" ] || continue
      "reply_$host" "$tid" "$cid" "$text"
      replied=$((replied + 1))
    done < <(pending_replies)
    echo "pr=$pr replied=$replied skipped=$((skipped - replied))"
    ;;
esac
