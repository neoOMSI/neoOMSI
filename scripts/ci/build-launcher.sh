#!/usr/bin/env bash
# Build the launcher (github.com/neoOMSI/launcher at the commit in scripts/launcher-ref) into
# the package: dist/<platform>/launcher, on macOS into neoOMSI.app. LAUNCHER_SRC=../launcher
# builds a local checkout as it is instead.
set -euo pipefail

platform="${1:?usage: build-launcher.sh <windows|macos|linux> <x64|arm64>}"
arch="${2:?usage: build-launcher.sh <windows|macos|linux> <x64|arm64>}"
root="$(cd "$(dirname "$0")/../.." && pwd)"

case "$platform" in
  windows) flag=--win ;;
  macos) flag=--mac ;;
  linux) flag=--linux ;;
  *) echo "unknown platform $platform" >&2; exit 1 ;;
esac

if [ -n "${LAUNCHER_SRC:-}" ]; then
  src="$(cd "$LAUNCHER_SRC" && pwd)"
else
  ref="${LAUNCHER_SHA:-$(tr -d '[:space:]' < "$root/scripts/launcher-ref")}"
  if ! [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then
    echo "Launcher ref must be a full 40-character commit SHA, found: '$ref'" >&2
    exit 1
  fi
  src="$root/target/neoomsi-launcher-src"
  rm -rf "$src"
  git init --quiet "$src"
  git -C "$src" fetch --quiet --depth 1 https://github.com/neoOMSI/launcher.git "$ref"
  git -C "$src" checkout --quiet --detach FETCH_HEAD
fi

cd "$src"
pnpm_version="$(node -p "require('./package.json').packageManager.split('@')[1]")"
pnpm() { npx --yes "pnpm@$pnpm_version" "$@"; }
pnpm install --frozen-lockfile
pnpm build
out="$src/release/$platform-$arch"
rm -rf "$out"
pnpm run package:app --platform "$platform" --arch "$arch" --out "$out"

cd "$root"
case "$platform" in
  macos)
    app=dist/macos/neoOMSI.app
    dest="$app/Contents/Resources/launcher"
    rm -rf "$dest"
    mkdir -p "$dest"
    ditto "$(ls -d "$out"/mac*/"neoOMSI Launcher.app")" "$dest/neoOMSI Launcher.app"
    # --deep does not reach into Resources: the nested app needs its own signature first
    codesign --force --deep --sign - "$dest/neoOMSI Launcher.app"
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"
    [ -f "$dest/neoOMSI Launcher.app/Contents/MacOS/neoOMSI Launcher" ] || {
      echo "::error::Packaged macOS launcher executable not found in $dest" >&2
      exit 1
    }
    ;;
  windows)
    dest="dist/windows/launcher"
    rm -rf "$dest"
    cp -R "$(ls -d "$out"/"${flag#--}"-*unpacked)" "$dest"
    [ -f "$dest/neoOMSI Launcher.exe" ] || {
      echo "::error::Packaged Windows launcher executable not found in $dest" >&2
      exit 1
    }
    ;;
  linux)
    dest="dist/linux/launcher"
    rm -rf "$dest"
    cp -R "$(ls -d "$out"/"${flag#--}"-*unpacked)" "$dest"
    [ -f "$dest/neoomsi-launcher-app" ] || {
      echo "::error::Packaged Linux launcher executable not found in $dest" >&2
      exit 1
    }
    ;;
esac
echo "launcher $(git -C "$src" rev-parse --short HEAD 2>/dev/null || echo "(local)") -> $dest"
