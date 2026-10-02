# Building from source

Releases for every commit are on the [Releases](https://github.com/neoOMSI/neoOMSI/releases)
page; build from source only to work on neoOMSI itself.

All scripts live in `scripts/`, run from any folder (paths with spaces are fine) and put the
result into `dist/<platform>/`. That folder is also the game's **content folder** (mods go
beside the binary), so the scripts replace only the binaries and never delete anything else
there.

## Requirements

* [Rust stable](https://rustup.rs), 1.85 or newer.
* **macOS**: Xcode Command Line Tools (`xcode-select --install`). Metal is used for drawing.
* **Windows**: Rust *x86_64 MSVC* and Visual Studio Build Tools with *Desktop development
  with C++* and the Windows SDK. CMake is needed to build the OpenXR dependency;
  it must be on `PATH`. Vulkan or DirectX 12 is used for drawing.
* **Linux** (Debian/Ubuntu names):
  `sudo apt install build-essential pkg-config libasound2-dev libudev-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libssl-dev`.
  Vulkan drivers (Mesa, NVIDIA) are needed to play.
* **Android**: the `aarch64-linux-android` Rust target, JDK 17, and an Android SDK with
  platform 34 or newer, build-tools and the NDK. `scripts/build-android.sh` looks for them
  through `android/env.sh` (`ANDROID_HOME`, `ANDROID_NDK_HOME`).

## Build

| Platform | Command | Result |
| --- | --- | --- |
| macOS | `scripts/build-macos.sh` | `dist/macos/neoOMSI.app` |
| Windows | `scripts\build-windows.cmd` | `dist\windows\neoomsi.exe`, `neoomsi-launcher.exe` |
| Windows, from a Mac | `scripts/build-windows-cross.sh` (needs `brew install mingw-w64`) | `dist/windows/` |
| Linux | `scripts/build-linux.sh` | `dist/linux/neoomsi`, `neoomsi-launcher`, `.desktop` file |
| Android | `scripts/build-android.sh` | `dist/android/neoOMSI-<version>.apk` |
| Dedicated server | `scripts/build-server.sh [folder]` | `dist/server/` with `start.sh` |
| 32-bit plugin host | `scripts/build-plugin-host.sh` | `dist/omsi-plugin-host32.exe` (see [PLUGINS.md](PLUGINS.md)) |

Plain cargo works too: `cargo build --release -p omsi-app` builds `target/release/neoomsi`.

## The programs

* `neoomsi` (`crates/omsi-app`) - the game. Started with no arguments it opens the launcher
  window; with arguments it starts a session directly (see [USER_GUIDE.md](USER_GUIDE.md));
  with `--server server.cfg` it is the dedicated server.
* `neoomsi-launcher` (`crates/omsi-launcher-core`) - the launcher's commands for a terminal
  (`neoomsi-launcher --cli maps`, `--cli install '{"path":"mod.zip"}'` …).
* `omsi-check` (`tools/omsi-check`) - loads every content file of an installation and reports
  what failed: `cargo run --release -p omsi-check -- "/path/to/OMSI 2"`.

## Icons

The supplied app icon at `assets/icons/app/neoOMSI-icon-outlined.png` is rendered by the SVG wrappers
`neoomsi.svg` and `neoomsi-macos.svg`; the macOS wrapper applies the standard margin. Generated
`neoomsi.ico` is embedded into Windows executables, `neoomsi.icns` is used by the macOS bundle,
and `neoomsi-256.png` is the window icon on Windows and Linux. Regenerate the platform icons
after changing the source artwork (needs `cargo install resvg`; macOS also needs `iconutil`,
and Python 3 with Pillow):

```sh
scripts/make-icons.sh
```

## Tests

```sh
cargo test --workspace
```

Some tests read an OMSI 2 installation (`OMSI_ROOT=/path/to/OMSI 2`) and skip themselves
when there is none.
