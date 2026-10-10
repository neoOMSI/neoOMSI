#!/bin/sh
# Build neoOMSI for macOS (Apple silicon or Intel) into dist/macos/neoOMSI.app.
# Pass --package-launcher to also bundle the pinned Electron launcher.
# Needs Rust (https://rustup.rs) and the Xcode Command Line Tools (xcode-select --install).
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
[ "$(uname -s)" = Darwin ] || { echo "Run this script on macOS." >&2; exit 1; }
xcode-select -p >/dev/null 2>&1 || { echo "Run xcode-select --install, then run this script again." >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "Install Rust from https://rustup.rs, then run this script again." >&2; exit 1; }
target="${neoomsi_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
version="${neoomsi_VERSION:-$(sh scripts/version.sh 2>/dev/null || echo 0.0.0)}"
export neoomsi_VERSION="$version"
cargo build --locked --release --target "$target" -p core -p legacy-launcher-core
out=dist/macos
app="$out/neoOMSI.app"
# (only the bundle is replaced: the folders beside it are the content folder with the mods)
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/release/neoomsi" "$app/Contents/MacOS/neoomsi"
cp "target/$target/release/neoomsi-launcher" "$app/Contents/MacOS/neoomsi-launcher"
cp assets/icons/app/neoomsi.icns "$app/Contents/Resources/neoomsi.icns"
sed -e "s/@VERSION@/$version/g" scripts/macos/Info.plist > "$app/Contents/Info.plist"
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true
if [ "${1:-}" = "--package-launcher" ]; then
  arch=arm64
  case "$target" in
    x86_64*) arch=x64 ;;
  esac
  scripts/ci/build-launcher.sh macos "$arch"
fi
printf '\nneoOMSI %s built. Play: open "%s/%s"\n' "$version" "$PWD" "$app"
