#!/bin/sh
# Build neoOMSI for Linux x86_64 into dist/linux.
# Pass --package-launcher to also bundle the pinned Electron launcher.
# Needs Rust and, on Debian/Ubuntu:
#   sudo apt install build-essential pkg-config libasound2-dev libudev-dev libgtk-3-dev \
#     libxkbcommon-dev libwayland-dev libssl-dev
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "Install Rust from https://rustup.rs, then run this script again." >&2; exit 1; }
export neoomsi_VERSION="${neoomsi_VERSION:-$(sh scripts/version.sh 2>/dev/null || echo 0.0.0)}"
cargo build --locked --release -p core -p legacy-launcher-core
out=dist/linux
mkdir -p "$out"   # (the folder is also the content folder: mods stay)
cp target/release/neoomsi target/release/neoomsi-launcher "$out/"
cp assets/icons/app/neoomsi-256.png "$out/neoomsi.png"
cp scripts/linux/neoomsi.desktop "$out/"
if [ "${1:-}" = "--package-launcher" ]; then
  scripts/ci/build-launcher.sh linux x64
fi
printf '\nneoOMSI %s built in %s\n' "$neoomsi_VERSION" "$out"
