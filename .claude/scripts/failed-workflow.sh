#!/usr/bin/env bash
# Print only the error output of the most recent failed CI run on the
# current branch. Backend auto-detected: GitHub Actions
# (.github/workflows/*.yml) or Bitbucket Pipelines (bitbucket-pipelines.yml).
# Usage: failed-workflow.sh [branch]
set -euo pipefail

branch="${1:-$(git rev-parse --abbrev-ref HEAD)}"

if ls .github/workflows/*.y*ml >/dev/null 2>&1; then
  run_id=$(gh run list --branch "$branch" --status failure --limit 1 --json databaseId --jq '.[0].databaseId')
  if [ -z "${run_id:-}" ] || [ "$run_id" = "null" ]; then
    echo "No failed run found for branch '$branch'" >&2
    exit 1
  fi

  job_id=$(gh run view "$run_id" --json jobs --jq '.jobs[] | select(.conclusion=="failure") | .databaseId' | head -n1)
  if [ -z "${job_id:-}" ]; then
    echo "Run $run_id failed but no failing job found" >&2
    exit 1
  fi

  log=$(gh run view "$run_id" --job "$job_id" --log-failed 2>/dev/null || true)
  if [ -z "$log" ]; then
    log=$(gh api "repos/{owner}/{repo}/actions/jobs/$job_id/logs")
  fi

  grep -iE 'error|failed|failure' <<<"$log" | grep -viE '^.*Post job cleanup|deprecated'

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
  api="https://api.bitbucket.org/2.0/repositories/$workspace/$repo/pipelines/"

  pipeline=$(curl -sf -u "$user:$pass" -G "$api" \
    --data-urlencode "sort=-created_on" \
    --data-urlencode "pagelen=1" \
    --data-urlencode "q=target.ref_name=\"$branch\" AND state.result.name=\"FAILED\"")
  uuid=$(jq -r '.values[0].uuid // empty' <<<"$pipeline")
  if [ -z "$uuid" ]; then
    echo "No failed pipeline found for branch '$branch'" >&2
    exit 1
  fi

  steps=$(curl -sf -g -u "$user:$pass" "$api${uuid}/steps/")
  step_uuid=$(jq -r '[.values[] | select(.state.result.name=="FAILED")][0].uuid // empty' <<<"$steps")
  if [ -z "$step_uuid" ]; then
    echo "Pipeline $uuid failed but no failing step found" >&2
    exit 1
  fi

  curl -sf -g -u "$user:$pass" "$api${uuid}/steps/${step_uuid}/log" | grep -iE 'error|failed|failure'

else
  echo "No .github/workflows/ or bitbucket-pipelines.yml found — can't detect CI backend" >&2
  exit 1
fi
