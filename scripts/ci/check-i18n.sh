#!/usr/bin/env bash
set -euo pipefail

ALLOWED="assets/i18n/en.json"
DIR="assets/i18n"
MARKER="<!-- i18n-check -->"

changed=$(git diff --no-renames --name-only "origin/${BASE_REF}...HEAD" -- "$DIR" \
  | grep -vxF "$ALLOWED" || true)

comment_id=$(gh api --paginate "repos/${REPO}/issues/${PR_NUMBER}/comments" \
  --jq ".[] | select(.user.type == \"Bot\" and (.body | contains(\"${MARKER}\"))) | .id" \
  | sed -n '1p' || true)

if [ -n "$changed" ]; then
  while IFS= read -r file; do
    echo "::error file=${file}::Only ${ALLOWED} may be changed in ${DIR}/"
  done <<< "$changed"

  list=$(sed 's/.*/- `&`/' <<< "$changed")
  body=$(printf '%s\n### ❌ Invalid changes in `%s`\n\nOnly `%s` may be modified in this directory. The following files must not be changed:\n\n%s\n\nPlease revert these changes and push again.\n' \
    "$MARKER" "$DIR" "$ALLOWED" "$list")

  if [ -n "$comment_id" ]; then
    gh api -X PATCH "repos/${REPO}/issues/comments/${comment_id}" -f body="$body" > /dev/null || true
  else
    gh api -X POST "repos/${REPO}/issues/${PR_NUMBER}/comments" -f body="$body" > /dev/null || true
  fi

  exit 1
fi

if [ -n "$comment_id" ]; then
  gh api -X DELETE "repos/${REPO}/issues/comments/${comment_id}" > /dev/null || true
fi