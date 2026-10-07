#!/usr/bin/env bash
# Package built artifacts for a given platform (macos or linux).
set -euo pipefail

PLATFORM="${1:-}"
ARCH="${2:-}"
VERSION="${neoomsi_VERSION:-${3:-}}"

if [ -z "$PLATFORM" ] || [ -z "$ARCH" ] || [ -z "$VERSION" ]; then
  echo "Usage: $0 <macos|linux> <arch> <version>"
  exit 1
fi

mkdir -p out

case "$PLATFORM" in
  macos)
    cp -r README.md LICENSE NOTICE LICENSES dist/macos/
    (cd dist/macos && ditto -c -k --sequesterRsrc . "../../out/neoOMSI-${VERSION}-macos-${ARCH}.zip")
    ;;
  linux)
    cp -r README.md LICENSE NOTICE LICENSES dist/linux/
    cp -r LICENSE NOTICE LICENSES dist/server/
    (cd dist/linux && zip -qr "../../out/neoOMSI-${VERSION}-linux-${ARCH}.zip" .)
    (cd dist/server && zip -qr "../../out/neoOMSI-${VERSION}-server-linux-${ARCH}.zip" .)
    ;;
  *)
    echo "Unknown platform: $PLATFORM"
    exit 1
    ;;
esac
