#!/usr/bin/env bash
# Verify that a pull request includes a valid changelog fragment or skip-changelog label.
set -euo pipefail

LABELS_JSON="${LABELS_JSON:-[]}"
EXPECTED_PR="${EXPECTED_PR:-}"
BASE_REF="${BASE_REF:-main}"

if echo "$LABELS_JSON" | grep -q '"skip-changelog"'; then
  echo "PR has skip-changelog label. Skipping."
  exit 0
fi

if [ -z "$EXPECTED_PR" ]; then
  echo "::error::EXPECTED_PR is not set."
  exit 1
fi

FRAGMENTS=$(git diff --name-only "origin/${BASE_REF}...HEAD" -- '.changes/*.md' | grep -v 'README.md' || true)

if [ -z "$FRAGMENTS" ]; then
  echo "::error::Missing changelog fragment. Add .changes/$EXPECTED_PR.<category>.md or apply the reviewer-approved 'skip-changelog' label."
  exit 1
fi

found=false
for f in $FRAGMENTS; do
  [ -f "$f" ] || continue
  base=$(basename "$f")

  if ! [[ "$base" =~ ^([0-9]+)\.(parity|fix|feature|performance|breaking|docs|internal)\.md$ ]]; then
    echo "::error::Invalid changelog fragment '$base'. See .changes/README.md."
    exit 1
  fi

  if [ "${BASH_REMATCH[1]}" != "$EXPECTED_PR" ]; then
    echo "::error::Fragment '$base' references PR #${BASH_REMATCH[1]}, but this is PR #$EXPECTED_PR."
    exit 1
  fi

  if [ ! -s "$f" ]; then
    echo "::error::Fragment '$base' is empty."
    exit 1
  fi

  if grep -qE '^[[:space:]]*(#|[-*][[:space:]])' "$f"; then
    echo "::error::Fragment '$base' must be one concise paragraph, without headings or lists."
    exit 1
  fi

  found=true
done

if [ "$found" != true ]; then
  echo "::error::No valid changelog fragment for PR #$EXPECTED_PR."
  exit 1
fi
