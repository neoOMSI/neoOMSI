#!/usr/bin/env bash
# Publish a release (stable or nightly) using the GitHub CLI.
set -euo pipefail

VERSION="${VERSION:-}"
PRERELEASE="${PRERELEASE:-true}"
GITHUB_REF_TYPE="${GITHUB_REF_TYPE:-}"
GITHUB_REF_NAME="${GITHUB_REF_NAME:-}"
GITHUB_SHA="${GITHUB_SHA:-}"
GITHUB_RUN_NUMBER="${GITHUB_RUN_NUMBER:-}"
GITHUB_RUN_ID="${GITHUB_RUN_ID:-}"
GITHUB_SERVER_URL="${GITHUB_SERVER_URL:-https://github.com}"
GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-neoOMSI/neoOMSI}"

if [ -z "$VERSION" ]; then
  echo "::error::VERSION environment variable is required."
  exit 1
fi

# For git tags, use the tag name directly.
# For nightlies/snapshots, publish under the version tag (e.g. v0.2.0-nightly.g<sha>).
if [ "$GITHUB_REF_TYPE" = "tag" ]; then
  TAG="$GITHUB_REF_NAME"
else
  TAG="v${VERSION}"
fi

echo "Publishing release with tag $TAG for version $VERSION"

ls -l out

if [ "$GITHUB_REF_TYPE" = "tag" ]; then
  # Stable/RC releases prefer the prepared CHANGELOG section. If release
  # preparation has not compiled it yet, fall back to all pending fragments.
  awk -v v="## $VERSION" 'index($0, v) == 1 && (length($0) == length(v) || substr($0, length(v) + 1, 1) == " ") {f = 1; next} f && /^## / {exit} f {print}' CHANGELOG.md > changes.md
  if ! grep -q '[^[:space:]]' changes.md; then
    bash scripts/render-changelog-fragments.sh changes.md
  fi
else
  # Nightlies show fragments changed since the latest previous release/tag or pending fragments.
  # (only version tags: the passenger pack's release has a tag of its own)
  previous_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
  if [ -n "$previous_tag" ] && git rev-parse --verify --quiet "refs/tags/$previous_tag" >/dev/null; then
    previous_commit="$(git rev-parse "refs/tags/$previous_tag")"
    fragments=()
    while IFS= read -r fragment; do
      [ "$fragment" = ".changes/README.md" ] && continue
      [ -f "$fragment" ] && fragments+=("$fragment")
    done < <(git diff --name-only "$previous_commit" "$GITHUB_SHA" -- '.changes/*.md')

    if [ "${#fragments[@]}" -gt 0 ]; then
      bash scripts/render-changelog-fragments.sh changes.md "${fragments[@]}"
    else
      : > changes.md
    fi
  else
    bash scripts/render-changelog-fragments.sh changes.md
  fi
fi

if ! grep -q '[^[:space:]]' changes.md; then
  if [ "$GITHUB_REF_TYPE" = "tag" ]; then
    echo "- Small changes and fixes." > changes.md
  else
    echo "- No notable user-facing changes since the previous nightly." > changes.md
  fi
fi

download_base="$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/releases/download/$TAG"
LAUNCHER_SHA="$(tr -d '[:space:]' < scripts/launcher-ref)"

cat > notes.md <<NOTES
> **Early development build.** Expect bugs. An original OMSI 2 installation is required; neoOMSI does not include game content.

**Source:** [\`${GITHUB_SHA:0:8}\`]($GITHUB_SERVER_URL/$GITHUB_REPOSITORY/commit/$GITHUB_SHA) · **Launcher:** [\`${LAUNCHER_SHA:0:8}\`](https://github.com/neoOMSI/launcher/commit/$LAUNCHER_SHA) · **Build:** [#${GITHUB_RUN_NUMBER}]($GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID)

## What's changed

NOTES
cat changes.md >> notes.md
cat >> notes.md <<NOTES

## Downloads

| Platform | Game | Dedicated server |
| --- | --- | --- |
| Windows x64 | [\`neoOMSI-$VERSION-windows-x64.zip\`]($download_base/neoOMSI-$VERSION-windows-x64.zip) | [\`neoOMSI-$VERSION-server-windows-x64.zip\`]($download_base/neoOMSI-$VERSION-server-windows-x64.zip) |
| Windows ARM64 | [\`neoOMSI-$VERSION-windows-arm64.zip\`]($download_base/neoOMSI-$VERSION-windows-arm64.zip) | [\`neoOMSI-$VERSION-server-windows-arm64.zip\`]($download_base/neoOMSI-$VERSION-server-windows-arm64.zip) |
| macOS (Apple silicon) | [\`neoOMSI-$VERSION-macos-arm64.zip\`]($download_base/neoOMSI-$VERSION-macos-arm64.zip) | — |
| macOS (Intel) | [\`neoOMSI-$VERSION-macos-x64.zip\`]($download_base/neoOMSI-$VERSION-macos-x64.zip) | — |
| Linux x64 | [\`neoOMSI-$VERSION-linux-x64.zip\`]($download_base/neoOMSI-$VERSION-linux-x64.zip) | [\`neoOMSI-$VERSION-server-linux-x64.zip\`]($download_base/neoOMSI-$VERSION-server-linux-x64.zip) |
| Linux ARM64 | [\`neoOMSI-$VERSION-linux-arm64.zip\`]($download_base/neoOMSI-$VERSION-linux-arm64.zip) | [\`neoOMSI-$VERSION-server-linux-arm64.zip\`]($download_base/neoOMSI-$VERSION-server-linux-arm64.zip) |
NOTES

if [ "$PRERELEASE" = "true" ]; then
  flag="--prerelease"
else
  flag="--prerelease=false"
fi

if gh release view "$TAG" >/dev/null 2>&1; then
  gh release upload "$TAG" out/*.zip --clobber
  gh release edit "$TAG" --title "neoOMSI $VERSION" --notes-file notes.md "$flag"
else
  gh release create "$TAG" out/*.zip \
    --target "$GITHUB_SHA" \
    --title "neoOMSI $VERSION" \
    --notes-file notes.md \
    "$flag"
fi

echo "Successfully published neoOMSI $VERSION under tag $TAG."
