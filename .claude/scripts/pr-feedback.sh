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
#   pr-feedback.sh reply <number> <file> [<stage> <sha>]
#     For every thread in <file> whose `reply:` block is filled, posts it on
#     the thread. `reply: fixed` posts "Fixed in <stage> (<sha>)" plus the
#     block's bullets and resolves the thread — <stage> and <sha> (the pushed
#     head) are required when any thread is `fixed`. `reply: declined` posts
#     "Not changed — <first bullet>" plus the remaining bullets and leaves
#     the thread open for the user. An empty `reply:` is left untouched. Prints
#       pr=<n> fixed=<count> declined=<count> open=<count>
#
# File shape written by `fetch` (agents fill the `reply:` block only, never
# the rest; bullets are two-space-indented `- ` lines directly under it):
#
#   # PR feedback — fetched 2026-01-02T14:02
#   ## thread <thread-id>
#   comment: <first-comment-id>
#   path: src/x.ext:42
#   [alice @ 2026-01-02T13:50]
#   <comment body, verbatim>
#   [bob @ 2026-01-02T13:55]
#   <comment body, verbatim>
#   reply: fixed
#     - `App::clone_name`: probe starts at N=2
#     - test: `ut_clone_name_lowest_free_suffix`
set -euo pipefail

usage() {
  echo "Usage: pr-feedback.sh fetch <number> <out-file> | reply <number> <file> [<stage> <sha>]" >&2
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
# Emits "thread-id<TAB>comment-id<TAB>kind<TAB>bullets" per filled thread;
# bullets are the block's `- ` lines without the marker, joined by \x1f.
pending_replies() {
  awk '
    function flush() { if (kind != "") print tid "\t" cid "\t" kind "\t" body; kind = "" }
    /^## thread /  { flush(); tid=$3; cid=""; body="" }
    /^comment: /   { cid=$2 }
    /^reply: (fixed|declined)$/ { kind=$2; inblock=1; next }
    /^reply:/      { inblock=0 }
    inblock && /^  - / { sub(/^  - /, ""); body = body (body == "" ? "" : "\037") $0; next }
    inblock && !/^  - / { inblock=0 }
    END { flush() }
  ' "$file"
}

# Builds the posted comment: first line by kind, then one `- ` line per bullet.
reply_body() {
  local kind="$1" bullets="$2" first rest
  case "$kind" in
    fixed)
      printf 'Fixed in %s (%s)\n' "$stage" "$sha"
      rest="$bullets" ;;
    declined)
      first="${bullets%%$'\037'*}"
      printf 'Not changed — %s\n' "$first"
      rest="${bullets#"$first"}"; rest="${rest#$'\037'}" ;;
  esac
  [ -n "$rest" ] && printf '%s\n' "$rest" | tr '\037' '\n' | sed 's/^/- /'
  return 0
}

reply_github() {
  local tid="$1" cid="$2" kind="$3" text="$4"
  gh api "repos/{owner}/{repo}/pulls/$pr/comments/$cid/replies" -f body="$text" >/dev/null
  [ "$kind" = fixed ] || return 0
  gh api graphql -F id="$tid" -f query='
    mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }' >/dev/null
}

reply_bitbucket() {
  local tid="$1" cid="$2" kind="$3" text="$4"
  jq -n --arg t "$text" --argjson p "$cid" '{content: {raw: $t}, parent: {id: $p}}' \
    | curl -sf -u "$user:$pass" -H 'Content-Type: application/json' -d @- "$api/comments" >/dev/null
  [ "$kind" = fixed ] || return 0
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
    stage="${4:-}"; sha="${5:-}"
    if grep -q '^reply: fixed$' "$file" && { [ -z "$stage" ] || [ -z "$sha" ]; }; then
      echo "reply: a 'reply: fixed' thread needs <stage> <sha> (the pushed head)" >&2; exit 1
    fi
    fixed=0; declined=0; total=$(grep -c '^## thread ' "$file" || true)
    while IFS=$'\t' read -r tid cid kind bullets; do
      [ -n "$tid" ] || continue
      "reply_$host" "$tid" "$cid" "$kind" "$(reply_body "$kind" "$bullets")"
      case "$kind" in fixed) fixed=$((fixed + 1)) ;; declined) declined=$((declined + 1)) ;; esac
    done < <(pending_replies)
    echo "pr=$pr fixed=$fixed declined=$declined open=$((total - fixed))"
    ;;
esac
