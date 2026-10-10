# Building from source

Prebuilt binaries for every commit on `main` are available on the [Releases](https://github.com/neoOMSI/neoOMSI/releases) page. Building from source is primarily needed when developing or testing changes locally.

All build scripts are located in `scripts/`. Output binaries are placed into `dist/<platform>/`.

## Prerequisites

- **Rust:** [Rust stable](https://rustup.rs) (1.85 or newer).
- **Windows:**
  - Rust `x86_64-pc-windows-msvc` toolchain.
  - Visual Studio Build Tools with the _Desktop development with C++_ workload and Windows SDK.
  - CMake on `PATH` (required for OpenXR).
- **macOS:**
  - Xcode Command Line Tools (`xcode-select --install`).
  - Metal is used as the rendering backend.
- **Linux (Debian/Ubuntu):**
  - `sudo apt install build-essential pkg-config libasound2-dev libudev-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libssl-dev`
  - Vulkan drivers (Mesa, NVIDIA, etc.).
- **Android:**
  - `aarch64-linux-android` Rust target.
  - JDK 17, Android SDK (API 34+), NDK, and `build-tools`.

## Development builds

For fast local testing during active development, use the development scripts:

- **Windows:** `scripts\dev-windows.cmd`
- **Windows (dev release):** `scripts\dev-windows-release.cmd` (optimized build without slow final LTO passes)
- **macOS:** `sh scripts/dev-macos.sh`

Pass additional game arguments directly:

```cmd
scripts\dev-windows.cmd --map maps/Grundorf/global.cfg
```

## Release builds

| Platform             | Command                            | Output                                             |
| -------------------- | ---------------------------------- | -------------------------------------------------- |
| **Windows**          | `scripts\build-windows.cmd`        | `dist\windows\neoomsi.exe`, `neoomsi-launcher.exe` |
| **macOS**            | `scripts/build-macos.sh`           | `dist/macos/neoOMSI.app`                           |
| **Linux**            | `scripts/build-linux.sh`           | `dist/linux/neoomsi`, `neoomsi-launcher`           |
| **Android**          | `scripts/build-android.sh`         | `dist/android/neoOMSI-<version>.apk`               |
| **Dedicated server** | `scripts/build-server.sh [folder]` | `dist/server/` with `start.sh`                     |

Release archives also carry the launcher ([neoOMSI/launcher](https://github.com/neoOMSI/launcher),
Electron), into `dist/<platform>/launcher`. Nightly CI selects the latest launcher `main`
commit once per build; tagged release candidates and stable releases use `scripts/launcher-ref`.
(on macOS into `neoOMSI.app/Contents/Resources/launcher`).

To package the launcher in local builds:
- Pass `--package-launcher` to `scripts/build-windows.cmd`, `scripts/build-linux.sh`, or `scripts/build-macos.sh`.
- Or run `scripts/ci/build-launcher.sh` directly (requires Node 24+ and pnpm):

```sh
bash scripts/ci/build-launcher.sh windows x64                          # the pinned commit
LAUNCHER_SRC=../launcher bash scripts/ci/build-launcher.sh windows x64 # a local checkout
```

Direct Cargo compilation is also supported:

```sh
cargo build --release -p core
```

## Binaries

- `neoomsi` (`../crates/core`) - The main simulator executable. Without arguments, it starts the external launcher shipped beside it (`dist/<platform>/launcher`). Running `--control-protocol` serves the launcher over stdin/stdout ([LAUNCHER_PROTOCOL.md](LAUNCHER_PROTOCOL.md)).
- `neoomsi-launcher` (`../crates/legacy-launcher-core`) - Starts the external launcher when invoked without arguments; with `--cli` provides a command-line interface for headless management, mod installation, and asset operations.
- `omsi-check` (`tools/omsi-check`) - Validation utility that verifies content integrity against an OMSI 2 installation.

## Running tests

Run the workspace test suite:

```sh
cargo nextest run --workspace
```

Integration tests that inspect original game content read the path from the `OMSI_ROOT` environment variable and skip automatically if omitted:

```sh
OMSI_ROOT="/path/to/OMSI 2" cargo nextest run --workspace
```
