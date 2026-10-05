#!/usr/bin/env bash
# Classify newly opened/reopened issues and close inactive needs-info issues.
set -euo pipefail

MODE="${1:-classify}"
REPO="${REPO:-${GITHUB_REPOSITORY:-}}"

if [ "$MODE" = "classify" ]; then
  ISSUE="${ISSUE:-${1:-}}"
  if [ -z "$ISSUE" ]; then
    echo "::error::Missing ISSUE number for classify mode."
    exit 1
  fi

  labels="$(gh issue view "$ISSUE" --repo "$REPO" --json labels --jq '.labels[].name')"
  if printf '%s\n' "$labels" | grep -q '^status: '; then
    echo "Issue already has a status label."
    exit 0
  fi

  gh issue edit "$ISSUE" --repo "$REPO" --add-label "status: untriaged"
  echo "Added status: untriaged to #$ISSUE."

elif [ "$MODE" = "clean-needs-info" ]; then
  cutoff="$(date -u -d '14 days ago' +%s)"

  gh issue list \
    --repo "$REPO" \
    --state open \
    --label "status: needs-info" \
    --limit 1000 \
    --json number,updatedAt,labels \
    --jq '.[] | [.number, .updatedAt, ([.labels[].name] | join("|"))] | @tsv' |
  while IFS=$'\t' read -r number updated labels; do
    [ -n "$number" ] || continue

    if [[ "|$labels|" == *"|triage: protected|"* ]]; then
      echo "#$number is protected."
      continue
    fi

    updated_epoch="$(date -u -d "$updated" +%s)"
    if (( updated_epoch > cutoff )); then
      continue
    fi

    gh issue comment "$number" \
      --repo "$REPO" \
      --body "Closing this report because the requested information has not been provided for 14 days. If the missing reproduction details, logs, or comparison evidence become available, please ask for the issue to be reopened or file a new report with that information."

    gh issue close "$number" \
      --repo "$REPO" \
      --reason "not planned"
  done
else
  echo "Unknown mode: $MODE (expected 'classify' or 'clean-needs-info')"
  exit 1
fi
