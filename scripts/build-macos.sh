#!/bin/sh
# Build neoOMSI for macOS (Apple silicon or Intel) into dist/macos/neoOMSI.app.
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
cargo build --locked --release --target "$target" -p core
out=dist/macos
app="$out/neoOMSI.app"
# (only the bundle is replaced: the folders beside it are the content folder with the mods)
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/release/neoomsi" "$app/Contents/MacOS/neoomsi"
cp assets/icons/app/neoomsi.icns "$app/Contents/Resources/neoomsi.icns"
sed -e "s/@VERSION@/$version/g" scripts/macos/Info.plist > "$app/Contents/Info.plist"
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true
printf '\nneoOMSI %s built. Play: open "%s/%s"\n' "$version" "$PWD" "$app"
