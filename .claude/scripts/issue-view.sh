#!/usr/bin/env bash
# Print an issue's title, body, and comments in compact plain text. Backend
# auto-detected: Jira (.claude/jira.local.json present) or GitHub (gh CLI).
# Usage: issue-view.sh <number|url>   (GitHub)
#        issue-view.sh <ISSUE-KEY>    (Jira)
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: issue-view.sh <number|url|ISSUE-KEY>" >&2
  exit 1
fi
issue="$1"

jira_creds=.claude/jira.local.json

if [ -f "$jira_creds" ]; then
  base_url=$(jq -r .baseUrl "$jira_creds")
  email=$(jq -r .email "$jira_creds")
  token=$(jq -r .apiToken "$jira_creds")

  issue_json=$(curl -sf -u "$email:$token" \
    "$base_url/rest/api/3/issue/$issue?fields=summary,status,reporter,description")
  comments_json=$(curl -sf -u "$email:$token" \
    "$base_url/rest/api/3/issue/$issue/comment?orderBy=created")

  jq -n --argjson issue "$issue_json" --argjson comments "$comments_json" '
    "\($issue.key) \($issue.fields.summary) [\($issue.fields.status.name)]",
    "by \($issue.fields.reporter.displayName // "unknown")",
    "",
    ([$issue.fields.description | .. | .text? // empty] | join(" ")),
    "",
    "--- \($comments.comments | length) comment(s) ---",
    ($comments.comments[] | "", "[\(.author.displayName) @ \(.created)]", ([.body | .. | .text? // empty] | join(" ")))
  '

else
  gh issue view "$issue" --json number,title,state,url,author,body,comments --jq '
    "#\(.number) \(.title) [\(.state)]",
    .url,
    "by \(.author.login)",
    "",
    (.body // "(no body)"),
    "",
    "--- \(.comments | length) comment(s) ---",
    (.comments[] | "", "[\(.author.login) @ \(.createdAt)]", (.body // "(empty)"))
  '
fi
