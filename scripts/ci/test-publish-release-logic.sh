#!/usr/bin/env bash
# Focused unit tests for publish-release.sh logic without touching GitHub.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
publish_script="$script_dir/publish-release.sh"

test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT

mkdir -p "$test_dir/mock-bin" "$test_dir/repo/out" "$test_dir/repo/scripts"
export PATH="$test_dir/mock-bin:$PATH"

# Setup dummy git repo
cd "$test_dir/repo"
git init --quiet
git config user.email "test@example.com"
git config user.name "Test"
echo "0e548aa77fea541b61e5f72e397473b1eef589af" > scripts/launcher-ref
cat > scripts/render-changelog-fragments.sh <<'EOF'
#!/usr/bin/env bash
echo "- Changes." > "$1"
EOF
chmod +x scripts/render-changelog-fragments.sh
touch .changes_dummy
git add .
git commit -m "init" --quiet

export GITHUB_SHA="0123456789abcdef0123456789abcdef01234567"
export GITHUB_RUN_NUMBER="42"
export GITHUB_RUN_ID="123456"
export GITHUB_SERVER_URL="https://github.com"
export GITHUB_REPOSITORY="neoOMSI/neoOMSI"
export VERSION="0.2.0-test"
export PRERELEASE="true"
export GITHUB_REF_TYPE="branch"

# Helper to create all 11 expected artifacts
create_all_artifacts() {
  for name in \
    "neoOMSI-0.2.0-test-windows-x64.zip" \
    "neoOMSI-0.2.0-test-server-windows-x64.zip" \
    "neoOMSI-0.2.0-test-windows-arm64.zip" \
    "neoOMSI-0.2.0-test-server-windows-arm64.zip" \
    "neoOMSI-0.2.0-test-macos-arm64.zip" \
    "neoOMSI-0.2.0-test-macos-x64.zip" \
    "neoOMSI-0.2.0-test-linux-x64.zip" \
    "neoOMSI-0.2.0-test-server-linux-x64.zip" \
    "neoOMSI-0.2.0-test-linux-arm64.zip" \
    "neoOMSI-0.2.0-test-server-linux-arm64.zip" \
    "build-manifest.json"; do
    touch "out/$name"
  done
}

# Test 0: Missing local artifacts -> fails immediately before any gh call
rm -rf out/*
touch out/neoOMSI-0.2.0-test-windows-x64.zip
if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 0 failed: expected failure when out/ does not contain all 11 artifacts" >&2
  exit 1
fi
echo "Test 0 passed: missing local artifacts fail before publishing."

create_all_artifacts

# Test 1: Release does not exist -> creates draft, then edits to publish
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $@" >> "$MOCK_LOG"
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  exit 1
fi
if [ "$1" = "release" ] && [ "$2" = "create" ]; then
  exit 0
fi
if [ "$1" = "release" ] && [ "$2" = "edit" ]; then
  exit 0
fi
exit 0
EOF
chmod +x "$test_dir/mock-bin/gh"

export MOCK_LOG="$test_dir/mock1.log"
bash "$publish_script" >/dev/null
if ! grep -q "release create.*--draft" "$MOCK_LOG" || ! grep -q "release edit.*--draft=false" "$MOCK_LOG"; then
  echo "Test 1 failed: expected create --draft then edit --draft=false" >&2
  exit 1
fi
echo "Test 1 passed: new release creates draft then finalizes."

# Test 2: Draft release exists -> uploads assets and publishes
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $@" >> "$MOCK_LOG"
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  echo '{"isDraft": true, "assets": []}'
  exit 0
fi
exit 0
EOF

export MOCK_LOG="$test_dir/mock2.log"
bash "$publish_script" >/dev/null
if ! grep -q "release upload" "$MOCK_LOG" || ! grep -q "release edit.*--draft=false" "$MOCK_LOG"; then
  echo "Test 2 failed: expected upload then edit --draft=false" >&2
  exit 1
fi
echo "Test 2 passed: draft release uploads assets then finalizes."

# Test 3: Published immutable release exists with all 11 assets -> succeeds idempotently
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $@" >> "$MOCK_LOG"
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  cat <<JSON
{
  "isDraft": false,
  "targetCommitish": "0123456789abcdef0123456789abcdef01234567",
  "assets": [
    {"name": "neoOMSI-0.2.0-test-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-arm64.zip"},
    {"name": "build-manifest.json"}
  ]
}
JSON
  exit 0
fi
if [ "$1" = "api" ]; then
  echo '{"sha": "0123456789abcdef0123456789abcdef01234567"}'
  exit 0
fi
if [ "$1" = "release" ] && [ "$2" = "download" ]; then
  target_dir="."
  for ((i=1; i<=$#; i++)); do
    if [ "${!i}" = "-D" ]; then
      next=$((i+1))
      target_dir="${!next}"
    fi
  done
  cat > "$target_dir/build-manifest.json" <<MANIFEST
{
  "version": "${MOCK_MANIFEST_VERSION:-0.2.0-test}",
  "channel": "${MOCK_MANIFEST_CHANNEL:-nightly}",
  "engine": "0123456789abcdef0123456789abcdef01234567",
  "launcher": "0e548aa77fea541b61e5f72e397473b1eef589af"
}
MANIFEST
  exit 0
fi
exit 0
EOF

export MOCK_LOG="$test_dir/mock3.log"
bash "$publish_script" >/dev/null
if grep -q "release create" "$MOCK_LOG" || grep -q "release edit" "$MOCK_LOG" || grep -q "release upload" "$MOCK_LOG"; then
  echo "Test 3 failed: immutable release should not call create, edit or upload" >&2
  exit 1
fi
echo "Test 3 passed: published immutable release is verified idempotently without mutation."
cp "$test_dir/mock-bin/gh" "$test_dir/mock-success-gh"

# Test 4: Published immutable release exists but is missing an asset -> fails
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  echo '{"isDraft": false, "targetCommitish": "0123456789abcdef0123456789abcdef01234567", "assets": [{"name": "neoOMSI-0.2.0-test-windows-x64.zip"}]}'
  exit 0
fi
exit 0
EOF

if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 4 failed: expected failure when immutable release is missing assets" >&2
  exit 1
fi
echo "Test 4 passed: missing assets on immutable release fail with error."

# Test 5: A resolved tag pointing to another full commit must be rejected
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  echo '{"isDraft": false, "targetCommitish": "main", "assets": []}'
  exit 0
fi
if [ "$1" = "api" ]; then
  echo '{"sha": "ffffffffffffffffffffffffffffffffffffffff"}'
  exit 0
fi
exit 0
EOF

if output="$(bash "$publish_script" 2>&1)"; then
  echo "Test 5 failed: mismatching resolved tag commit was accepted" >&2
  exit 1
fi
if ! grep -q "points to commit ffffffffffffffffffffffffffffffffffffffff" <<< "$output"; then
  echo "Test 5 failed for the wrong reason: $output" >&2
  exit 1
fi
echo "Test 5 passed: mismatching resolved tag commit is rejected."

# Test 6: Verify get_json_field boolean false distinguishes correctly from empty/missing
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  # isDraft is false, so it must not be treated as a draft!
  echo '{"isDraft": false, "targetCommitish": "main"}'
  exit 0
fi
if [ "$1" = "api" ]; then
  # commits/tag resolves to GITHUB_SHA
  echo '{"sha": "0123456789abcdef0123456789abcdef01234567"}'
  exit 0
fi
echo "gh $@" >> "$MOCK_LOG"
exit 0
EOF

export MOCK_LOG="$test_dir/mock6.log"
touch "$MOCK_LOG"
# Should fail because assets are missing, NOT proceed to upload as draft!
if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 6 failed: should fail due to missing assets on published release" >&2
  exit 1
fi
if grep -q "release upload" "$MOCK_LOG" || grep -q "release edit.*--draft=false" "$MOCK_LOG"; then
  echo "Test 6 failed: isDraft: false was misclassified as draft!" >&2
  exit 1
fi
echo "Test 6 passed: isDraft: false correctly prevents draft upload mutations."

# Test 7: Published immutable release exists with all assets but mismatching manifest -> fails
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  cat <<JSON
{
  "isDraft": false,
  "targetCommitish": "0123456789abcdef0123456789abcdef01234567",
  "assets": [
    {"name": "neoOMSI-0.2.0-test-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-arm64.zip"},
    {"name": "build-manifest.json"}
  ]
}
JSON
  exit 0
fi
if [ "$1" = "release" ] && [ "$2" = "download" ]; then
  # Download a manifest with a mismatching engine SHA
  target_dir="."
  for ((i=1; i<=$#; i++)); do
    if [ "${!i}" = "-D" ]; then
      next=$((i+1))
      target_dir="${!next}"
    fi
  done
  cat > "$target_dir/build-manifest.json" <<MANIFEST
{
  "version": "0.2.0-test",
  "channel": "nightly",
  "engine": "ffffffffffffffffffffffffffffffffffffffff",
  "launcher": "0e548aa77fea541b61e5f72e397473b1eef589af"
}
MANIFEST
  exit 0
fi
exit 0
EOF

if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 7 failed: expected failure when immutable release manifest has mismatching engine SHA" >&2
  exit 1
fi
echo "Test 7 passed: mismatching build manifest on immutable release fails closed."

# Test 8: Published immutable release where build-manifest.json download fails -> fails closed
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  cat <<JSON
{
  "isDraft": false,
  "targetCommitish": "0123456789abcdef0123456789abcdef01234567",
  "assets": [
    {"name": "neoOMSI-0.2.0-test-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-arm64.zip"},
    {"name": "build-manifest.json"}
  ]
}
JSON
  exit 0
fi
if [ "$1" = "api" ]; then
  echo '{"sha": "0123456789abcdef0123456789abcdef01234567"}'
  exit 0
fi
if [ "$1" = "release" ] && [ "$2" = "download" ]; then
  # Simulate download failure
  exit 1
fi
exit 0
EOF

if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 8 failed: expected failure when manifest download fails" >&2
  exit 1
fi
echo "Test 8 passed: failed manifest download fails closed."

# Test 9: Published immutable release where build-manifest.json is malformed/empty -> fails closed
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  cat <<JSON
{
  "isDraft": false,
  "targetCommitish": "0123456789abcdef0123456789abcdef01234567",
  "assets": [
    {"name": "neoOMSI-0.2.0-test-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-arm64.zip"},
    {"name": "build-manifest.json"}
  ]
}
JSON
  exit 0
fi
if [ "$1" = "api" ]; then
  echo '{"sha": "0123456789abcdef0123456789abcdef01234567"}'
  exit 0
fi
if [ "$1" = "release" ] && [ "$2" = "download" ]; then
  target_dir="."
  for ((i=1; i<=$#; i++)); do
    if [ "${!i}" = "-D" ]; then
      next=$((i+1))
      target_dir="${!next}"
    fi
  done
  echo "not json" > "$target_dir/build-manifest.json"
  exit 0
fi
exit 0
EOF

if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 9 failed: expected failure when manifest is malformed" >&2
  exit 1
fi
echo "Test 9 passed: malformed manifest fails closed."

# Test 10: Published immutable release where manifest is missing launcher SHA -> fails closed
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  cat <<JSON
{
  "isDraft": false,
  "targetCommitish": "0123456789abcdef0123456789abcdef01234567",
  "assets": [
    {"name": "neoOMSI-0.2.0-test-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-windows-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-macos-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-x64.zip"},
    {"name": "neoOMSI-0.2.0-test-linux-arm64.zip"},
    {"name": "neoOMSI-0.2.0-test-server-linux-arm64.zip"},
    {"name": "build-manifest.json"}
  ]
}
JSON
  exit 0
fi
if [ "$1" = "api" ]; then
  echo '{"sha": "0123456789abcdef0123456789abcdef01234567"}'
  exit 0
fi
if [ "$1" = "release" ] && [ "$2" = "download" ]; then
  target_dir="."
  for ((i=1; i<=$#; i++)); do
    if [ "${!i}" = "-D" ]; then
      next=$((i+1))
      target_dir="${!next}"
    fi
  done
  cat > "$target_dir/build-manifest.json" <<MANIFEST
{
  "version": "0.2.0-test",
  "channel": "nightly",
  "engine": "0123456789abcdef0123456789abcdef01234567"
}
MANIFEST
  exit 0
fi
exit 0
EOF

if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 10 failed: expected failure when manifest is missing launcher SHA" >&2
  exit 1
fi
echo "Test 10 passed: incomplete manifest missing launcher SHA fails closed."

# Test 11: Tag commit cannot be resolved -> fails closed
cat > "$test_dir/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "release" ] && [ "$2" = "view" ]; then
  echo '{"isDraft": false, "targetCommitish": "main", "assets": []}'
  exit 0
fi
if [ "$1" = "api" ]; then
  # API fails to resolve commit
  exit 1
fi
exit 0
EOF

if bash "$publish_script" >/dev/null 2>&1; then
  echo "Test 11 failed: expected failure when tag commit cannot be resolved" >&2
  exit 1
fi
echo "Test 11 passed: unresolvable tag commit fails closed."

# Test 12: Immutable release manifest must match the requested version
cp "$test_dir/mock-success-gh" "$test_dir/mock-bin/gh"
export MOCK_MANIFEST_VERSION="0.2.0-other"
if output="$(bash "$publish_script" 2>&1)"; then
  echo "Test 12 failed: mismatching manifest version was accepted" >&2
  exit 1
fi
if ! grep -q "manifest version" <<< "$output"; then
  echo "Test 12 failed for the wrong reason: $output" >&2
  exit 1
fi
unset MOCK_MANIFEST_VERSION
echo "Test 12 passed: immutable release manifest version mismatch rejected."

# Test 13: Immutable release manifest must match the requested channel
export MOCK_MANIFEST_CHANNEL="rc"
if output="$(bash "$publish_script" 2>&1)"; then
  echo "Test 13 failed: mismatching manifest channel was accepted" >&2
  exit 1
fi
if ! grep -q "manifest channel" <<< "$output"; then
  echo "Test 13 failed for the wrong reason: $output" >&2
  exit 1
fi
unset MOCK_MANIFEST_CHANNEL
echo "Test 13 passed: immutable release manifest channel mismatch rejected."

echo "All release publishing tests passed!"
