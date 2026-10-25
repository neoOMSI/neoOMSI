# Building from source

Prebuilt development builds for supported desktop platforms are available on the [Releases](https://github.com/neoOMSI/neoOMSI/releases) page. Building from source is primarily needed when developing or testing changes locally.

**Only Windows, macOS and Linux desktop builds are supported. There is no Android or other mobile release.**

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
  - `sudo apt install build-essential pkg-config libasound2-dev libudev-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libssl-dev clang mold`
  - Without mold, build with `RUSTFLAGS="-C link-arg=-fuse-ld=lld"`.
  - Vulkan drivers (Mesa, NVIDIA, etc.).

## Development builds

For fast local testing during active development, use the development scripts:

- **Windows:** `scripts\dev-windows.cmd`
- **Windows (dev release):** `scripts\dev-windows-release.cmd` (optimized build without slow final LTO passes)
- **macOS:** `sh scripts/dev-macos.sh`

Pass additional game arguments directly:

```cmd
scripts\dev-windows.cmd --map maps/Grundorf/global.cfg
```

The optional DLSS/FSR runtimes have a separate, pinned SDK preparation step (see below). Normal game builds do not require these SDKs.

## Release builds

| Platform             | Command                            | Output                         |
| -------------------- | ---------------------------------- | ------------------------------ |
| **Windows**          | `scripts\build-windows.cmd`        | `dist\windows\neoomsi.exe`     |
| **macOS**            | `scripts/build-macos.sh`           | `dist/macos/neoOMSI.app`       |
| **Linux**            | `scripts/build-linux.sh`           | `dist/linux/neoomsi`           |
| **Dedicated server** | `scripts/build-server.sh [folder]` | `dist/server/` with `start.sh` |

Direct Cargo compilation is also supported:

```sh
cargo build --release -p core
```

## Optional DLSS and FSR runtimes (Windows)

Normal builds need no AMD or NVIDIA SDK and run without these files (the game falls back to
spatial upscaling). To build the bridges once, with Visual Studio 2022 or 2026 C++ Build Tools,
CMake 3.23+ (4.2+ for VS 2026) and Git:

```powershell
./scripts/prepare-upscale-sdk.ps1            # first run downloads the pinned SDKs
./scripts/prepare-upscale-sdk.ps1 -Offline   # later rebuilds
./scripts/prepare-upscale-sdk.ps1 -Validation  # also the Khronos validation layer for GPU tests
```

Pins and hashes are in `scripts/upscale-sdk-lock.json`. Everything lands in
`target/upscale-sdk/`; the files the game loads are in `target/upscale-sdk/runtime`. A
development build finds them there by itself; otherwise the game looks next to its exe or in
`OMSI_UPSCALE_RUNTIME`. The Windows x64 release (`scripts/ci/package-windows.ps1`) runs
`prepare-upscale-sdk.ps1 -NoProbe -Dest dist/windows`, which puts exactly these next to the
exe, plus NVIDIA's licence texts in `LICENSES/` (if that step fails, the release is built
without them):

| For  | Files                                                                                                                               |
|------|-------------------------------------------------------------------------------------------------------------------------------------|
| FSR  | `neo_fsr_dx12.dll`, `neo_fsr_vk.dll`                                                                                                |
| DLSS | `neo_dlss_dx12.dll`, `neo_dlss_vk.dll`, `sl.interposer.dll`, `sl.common.dll`, `sl.dlss.dll`, `NvLowLatencyVk.dll`, `nvngx_dlss.dll` |

Not for players: `neo_upscale_probe_*.dll`, `probe-data/`, the validation layer and the SDK
folders.

## Binaries

- `neoomsi` (`crates/core`) - The main simulator executable. Without arguments, it opens directly into the in-game start menu.
- `omsi-check` (`tools/omsi-check`) - Read-only content validation with dependency checks, JSON reports and optional runtime scenarios. See [Content validation](CONTENT_VALIDATION.md).

## Running tests

Run the workspace test suite:

```sh
cargo nextest run --workspace
```

Integration tests that inspect original game content read the path from the `OMSI_ROOT` environment variable and skip automatically if omitted:

```sh
OMSI_ROOT="/path/to/OMSI 2" cargo nextest run --workspace
```
