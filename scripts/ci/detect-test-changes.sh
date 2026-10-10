#!/usr/bin/env bash
# Detect whether changes between commits/refs require running workspace tests.
set -euo pipefail

EVENT_NAME="${EVENT_NAME:-}"
PR_BASE="${PR_BASE:-}"
PR_HEAD="${PR_HEAD:-}"
PUSH_BEFORE="${PUSH_BEFORE:-}"
PUSH_HEAD="${PUSH_HEAD:-}"
GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/null}"

if [ "$EVENT_NAME" = "workflow_dispatch" ]; then
  echo "run-tests=true" >> "$GITHUB_OUTPUT"
  echo "run-bundle=true" >> "$GITHUB_OUTPUT"
  echo "Manual run: tests and bundle check required."
  exit 0
fi

if [ "$EVENT_NAME" = "pull_request" ]; then
  range="$PR_BASE...$PR_HEAD"
elif [ "$EVENT_NAME" = "push" ]; then
  if [ -z "$PUSH_BEFORE" ] || [ "$PUSH_BEFORE" = "0000000000000000000000000000000000000000" ]; then
    echo "run-tests=true" >> "$GITHUB_OUTPUT"
    echo "run-bundle=true" >> "$GITHUB_OUTPUT"
    echo "No usable previous revision: tests and bundle check required."
    exit 0
  fi
  range="$PUSH_BEFORE..$PUSH_HEAD"
else
  echo "run-tests=true" >> "$GITHUB_OUTPUT"
  echo "run-bundle=true" >> "$GITHUB_OUTPUT"
  echo "Unknown event type: tests and bundle check required."
  exit 0
fi

echo "Checking changes in $range"
changed="$(git diff --name-only "$range")"

echo "Changed files:"
printf '%s\n' "$changed"

# Workspace tests check: Rust crates and workspace configs
if printf '%s\n' "$changed" | grep -Eq \
  '^(crates/|tools/|Cargo\.toml$|Cargo\.lock$|\.cargo/|rust-toolchain$|rust-toolchain\.toml$|\.github/workflows/test\.yml$|\.github/actions/)'
then
  echo "run-tests=true" >> "$GITHUB_OUTPUT"
  echo "Rust workspace test-relevant changes detected."
else
  echo "run-tests=false" >> "$GITHUB_OUTPUT"
  echo "No Rust workspace test-relevant changes detected."
fi

# Combined bundle check: engine code, scripts (build, dev, ci, launcher-ref), packaging workflows
if printf '%s\n' "$changed" | grep -Eq \
  '^(crates/|tools/|scripts/|Cargo\.toml$|Cargo\.lock$|\.cargo/|rust-toolchain$|rust-toolchain\.toml$|\.github/workflows/(test|build|build-target|pr-build)\.yml$|\.github/actions/)'
then
  echo "run-bundle=true" >> "$GITHUB_OUTPUT"
  echo "Combined bundle-relevant changes detected."
else
  echo "run-bundle=false" >> "$GITHUB_OUTPUT"
  echo "No combined bundle-relevant changes detected."
fi
