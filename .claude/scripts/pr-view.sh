#!/usr/bin/env bash
# Print a PR's title, body, and comments in compact plain text. Backend
# auto-detected: GitHub Actions (.github/workflows/*.yml, via `gh`) or
# Bitbucket Pipelines (bitbucket-pipelines.yml, via REST using
# .claude/bitbucket.local.json) — same detection as failed-workflow.sh.
#
# Requests an explicit field list instead of `gh pr view`'s default query,
# which currently errors on any repo carrying a legacy Projects-Classic
# board: "GraphQL: Projects (classic) is being deprecated ...
# (repository.pullRequest.projectCards)". Reproduces on `gh pr view <n>`
# and `gh pr view <n> --comments` alike, with or without `--comments`.
#
# Usage: pr-view.sh <number>
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: pr-view.sh <number>" >&2
  exit 1
fi
pr="$1"

if ls .github/workflows/*.y*ml >/dev/null 2>&1; then
  gh pr view "$pr" --json number,title,state,url,author,body,comments --jq '
    "#\(.number) \(.title) [\(.state)]",
    .url,
    "by \(.author.login)",
    "",
    (.body // "(no body)"),
    "",
    "--- \(.comments | length) comment(s) ---",
    (.comments[] | "", "[\(.author.login) @ \(.createdAt)]", (.body // "(empty)"))
  '

elif ls bitbucket-pipelines.y*ml >/dev/null 2>&1; then
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

  pr_json=$(curl -sf -u "$user:$pass" "$api")
  comments_json=$(curl -sf -u "$user:$pass" -G "$api/comments" --data-urlencode "pagelen=50")

  jq -n --argjson pr "$pr_json" --argjson comments "$comments_json" '
    "#\($pr.id) \($pr.title) [\($pr.state)]",
    $pr.links.html.href,
    "by \($pr.author.display_name // "unknown")",
    "",
    ($pr.description // "(no body)"),
    "",
    "--- \([$comments.values[] | select(.deleted|not)] | length) comment(s) ---",
    ($comments.values[] | select(.deleted|not) | "", "[\(.user.display_name // "unknown") @ \(.created_on)]", (.content.raw // "(empty)"))
  '

else
  echo "No .github/workflows/ or bitbucket-pipelines.yml found — can't detect PR host" >&2
  exit 1
fi
