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
LAUNCHER_SHA="${LAUNCHER_SHA:-$(tr -d '[:space:]' < scripts/launcher-ref)}"
CHANNEL="${CHANNEL:-$([ "$PRERELEASE" = "true" ] && echo "nightly" || echo "stable")}"

# Generate build-manifest.json machine-readable metadata
cat > out/build-manifest.json <<MANIFEST
{
  "version": "$VERSION",
  "channel": "$CHANNEL",
  "engine": "$GITHUB_SHA",
  "launcher": "$LAUNCHER_SHA",
  "buildNumber": "$GITHUB_RUN_NUMBER",
  "buildId": "$GITHUB_RUN_ID"
}
MANIFEST

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
| macOS (Apple silicon) | [\`neoOMSI-$VERSION-macos-arm64.zip\`]($download_base/neoOMSI-$VERSION-macos-arm64.zip) | - |
| macOS (Intel) | [\`neoOMSI-$VERSION-macos-x64.zip\`]($download_base/neoOMSI-$VERSION-macos-x64.zip) | - |
| Linux x64 | [\`neoOMSI-$VERSION-linux-x64.zip\`]($download_base/neoOMSI-$VERSION-linux-x64.zip) | [\`neoOMSI-$VERSION-server-linux-x64.zip\`]($download_base/neoOMSI-$VERSION-server-linux-x64.zip) |
| Linux ARM64 | [\`neoOMSI-$VERSION-linux-arm64.zip\`]($download_base/neoOMSI-$VERSION-linux-arm64.zip) | [\`neoOMSI-$VERSION-server-linux-arm64.zip\`]($download_base/neoOMSI-$VERSION-server-linux-arm64.zip) |
| Build Manifest | [\`build-manifest.json\`]($download_base/build-manifest.json) | - |
NOTES

if [ "$PRERELEASE" = "true" ]; then
  flag="--prerelease"
else
  flag="--prerelease=false"
fi

# Expected release artifacts advertised in release notes (6 desktop builds + 4 dedicated server builds + build-manifest.json)
expected_artifacts=(
  "neoOMSI-$VERSION-windows-x64.zip"
  "neoOMSI-$VERSION-server-windows-x64.zip"
  "neoOMSI-$VERSION-windows-arm64.zip"
  "neoOMSI-$VERSION-server-windows-arm64.zip"
  "neoOMSI-$VERSION-macos-arm64.zip"
  "neoOMSI-$VERSION-macos-x64.zip"
  "neoOMSI-$VERSION-linux-x64.zip"
  "neoOMSI-$VERSION-server-linux-x64.zip"
  "neoOMSI-$VERSION-linux-arm64.zip"
  "neoOMSI-$VERSION-server-linux-arm64.zip"
  "build-manifest.json"
)

missing_local=0
for expected in "${expected_artifacts[@]}"; do
  if [ ! -f "out/$expected" ]; then
    echo "::error::Expected release artifact missing from out/: $expected"
    missing_local=1
  fi
done

if [ "$missing_local" -ne 0 ]; then
  echo "::error::Incomplete release artifacts in out/. Cannot publish release without all advertised downloads."
  exit 1
fi

shopt -s nullglob
artifacts=(out/*.zip out/build-manifest.json)

get_json_field() {
  local json="$1"
  local field="$2"
  if command -v jq >/dev/null 2>&1; then
    echo "$json" | jq -r "if has(\"${field}\") and .\"${field}\" != null then .\"${field}\" else empty end"
  elif command -v node >/dev/null 2>&1; then
    node -e "const d=JSON.parse(process.argv[1]); const v=d[process.argv[2]]; console.log(v !== undefined && v !== null ? String(v) : '');" "$json" "$field"
  else
    echo ""
  fi
}

has_release_asset() {
  local json="$1"
  local asset_name="$2"
  if command -v jq >/dev/null 2>&1; then
    echo "$json" | jq -e --arg name "$asset_name" '.assets[] | select(.name == $name)' >/dev/null 2>&1
  elif command -v node >/dev/null 2>&1; then
    node -e "const d=JSON.parse(process.argv[1]); const found=Array.isArray(d.assets) && d.assets.some(a=>a.name===process.argv[2]); process.exit(found ? 0 : 1);" "$json" "$asset_name"
  else
    return 1
  fi
}

resolve_release_commit() {
  local tag="$1"
  # 1. If local git has the tag, resolve it directly (annotated tags dereference to commit)
  if git rev-parse --verify --quiet "refs/tags/$tag^{commit}" >/dev/null 2>&1; then
    git rev-parse "refs/tags/$tag^{commit}"
    return 0
  fi
  # 2. Query GitHub commit API for the tag (handles lightweight and annotated tags)
  local commit_json
  commit_json="$(gh api "/repos/$GITHUB_REPOSITORY/commits/$tag" 2>/dev/null || true)"
  local api_sha
  api_sha="$(get_json_field "$commit_json" "sha")"
  if [ -n "$api_sha" ] && [ "$api_sha" != "null" ]; then
    echo "$api_sha"
    return 0
  fi
  echo ""
}

if release_json="$(gh release view "$TAG" --json isDraft,isPrerelease,targetCommitish,assets 2>/dev/null)"; then
  is_draft="$(get_json_field "$release_json" "isDraft")"
  if [ "$is_draft" = "false" ]; then
    echo "Release $TAG is already published (immutable)."
    actual_commit="$(resolve_release_commit "$TAG")"
    if [ -z "$actual_commit" ]; then
      echo "::error::Could not resolve git commit for published release tag $TAG."
      exit 1
    fi
    if [ -n "$GITHUB_SHA" ] && [ "$actual_commit" != "$GITHUB_SHA" ]; then
      echo "::error::Published immutable release $TAG points to commit $actual_commit, but current commit is $GITHUB_SHA."
      exit 1
    fi

    missing_remote=0
    for expected in "${expected_artifacts[@]}"; do
      if ! has_release_asset "$release_json" "$expected"; then
        echo "::error::Required asset $expected is missing from published immutable release $TAG."
        missing_remote=1
      fi
    done
    if [ "$missing_remote" -ne 0 ]; then
      echo "::error::Cannot modify published immutable release $TAG. To update or fix a release, push a new commit or tag with a new version identifier."
      exit 1
    fi

    # Verify build-manifest.json content matches this build (strictly fail-closed)
    manifest_tmp="$(mktemp -d)"
    if ! gh release download "$TAG" -p "build-manifest.json" -D "$manifest_tmp" >/dev/null 2>&1 || [ ! -f "$manifest_tmp/build-manifest.json" ]; then
      rm -rf "$manifest_tmp"
      echo "::error::Failed to download build-manifest.json from published immutable release $TAG."
      exit 1
    fi

    remote_manifest="$(cat "$manifest_tmp/build-manifest.json")"
    rm -rf "$manifest_tmp"

    remote_engine="$(get_json_field "$remote_manifest" "engine")"
    remote_launcher="$(get_json_field "$remote_manifest" "launcher")"
    remote_version="$(get_json_field "$remote_manifest" "version")"
    remote_channel="$(get_json_field "$remote_manifest" "channel")"

    if [ "$remote_version" != "$VERSION" ]; then
      echo "::error::Published immutable release $TAG manifest version '$remote_version' does not match '$VERSION'."
      exit 1
    fi
    if [ "$remote_channel" != "$CHANNEL" ]; then
      echo "::error::Published immutable release $TAG manifest channel '$remote_channel' does not match '$CHANNEL'."
      exit 1
    fi

    if [ -z "$remote_engine" ] || ! [[ "$remote_engine" =~ ^[0-9a-f]{40}$ ]]; then
      echo "::error::Published immutable release $TAG manifest missing valid full engine commit SHA, found: '$remote_engine'."
      exit 1
    fi
    if [ -z "$remote_launcher" ] || ! [[ "$remote_launcher" =~ ^[0-9a-f]{40}$ ]]; then
      echo "::error::Published immutable release $TAG manifest missing valid full launcher commit SHA, found: '$remote_launcher'."
      exit 1
    fi

    if [ -n "$GITHUB_SHA" ] && [ "$remote_engine" != "$GITHUB_SHA" ]; then
      echo "::error::Published immutable release $TAG manifest records engine commit $remote_engine, but current commit is $GITHUB_SHA."
      exit 1
    fi
    if [ -n "$LAUNCHER_SHA" ] && [ "$remote_launcher" != "$LAUNCHER_SHA" ]; then
      echo "::error::Published immutable release $TAG manifest records launcher commit $remote_launcher, but current launcher is $LAUNCHER_SHA."
      exit 1
    fi

    echo "All 11 required artifacts and build manifest verified on immutable release $TAG. Verification succeeded."
    exit 0
  else
    echo "Found existing draft release $TAG. Uploading artifacts and publishing..."
    gh release upload "$TAG" "${artifacts[@]}" --clobber
    gh release edit "$TAG" --title "neoOMSI $VERSION" --notes-file notes.md "$flag" --draft=false
  fi
else
  echo "Creating draft release $TAG..."
  gh release create "$TAG" "${artifacts[@]}" \
    --target "$GITHUB_SHA" \
    --title "neoOMSI $VERSION" \
    --notes-file notes.md \
    --draft \
    "$flag"
  echo "Finalizing publication of release $TAG..."
  gh release edit "$TAG" --draft=false
fi

echo "Successfully published neoOMSI $VERSION under tag $TAG."
