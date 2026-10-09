#!/usr/bin/env bash
# Build the 32-bit Windows legacy plugin host.

set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="i686-pc-windows-gnu"
TARGET_DIR="target/legacy-plugin-win32"
OUTPUT="dist/legacy-plugin-host32.exe"

command -v i686-w64-mingw32-gcc >/dev/null || {
    echo "Missing MinGW i686 GCC toolchain" >&2
    exit 1
}

command -v rustup >/dev/null || {
    echo "Missing rustup" >&2
    exit 1
}

rustup target list --installed | grep -qx "$TARGET" || {
    echo "Missing Rust target: $TARGET" >&2
    exit 1
}

mkdir -p dist
rm -f "$OUTPUT"

export CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER=i686-w64-mingw32-gcc
export CARGO_TARGET_DIR="$TARGET_DIR"

BASE_FLAGS="-C panic=abort -C link-arg=-Wl,--enable-stdcall-fixup"

build_host() {
    cargo build \
        --locked \
        --release \
        --target "$TARGET" \
        -p legacy-plugin \
        --bin legacy-plugin-host
}

LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

echo "Building with MinGW unwind support..."

export RUSTFLAGS="$BASE_FLAGS --check-cfg=cfg(legacy_unwind_fallback)"

if build_host >"$LOG" 2>&1; then
    cat "$LOG"
else
    cat "$LOG" >&2

    # Only retry when the linker reports a missing
    # _Unwind_Resume symbol.
    if grep -Eiq \
        "(undefined reference to|undefined symbol:|unresolved external symbol).*_Unwind_Resume" \
        "$LOG"; then

        echo "MinGW cannot resolve _Unwind_Resume."
        echo "Retrying with the legacy fallback..."

        export RUSTFLAGS="$BASE_FLAGS --check-cfg=cfg(legacy_unwind_fallback) --cfg legacy_unwind_fallback"

        build_host
    else
        echo "Build failed for an unrelated reason." >&2
        exit 1
    fi
fi

cp "$TARGET_DIR/$TARGET/release/legacy-plugin-host.exe" \
   "$OUTPUT"

echo "Plugin host installed as $OUTPUT"