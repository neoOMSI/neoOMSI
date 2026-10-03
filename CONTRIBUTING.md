# Contributing to neoOMSI

Thanks for helping! A few rules keep the project healthy:

* **No original code or assets.** Never copy anything from an OMSI 2 installation into the
  repository - no textures, models, sounds, maps, scripts. Tests that need
  content read it from a local installation (`OMSI_ROOT`) and skip themselves without one.
* **Compatibility first.** A change must not break a stock map or a mod that worked. Run
  `cargo run --release -p omsi-check -- "/path/to/OMSI 2"` before and after larger changes.
* **One change per pull request**, with a message that says what the player notices.
* **Use a fork and a branch.** Fork neoOMSI, make a separate branch for your changes, then
  open a pull request from that branch.
* Code style: `rustfmt` defaults, comments explain *why*.

## Building while you work

Use the development build while you are changing and testing the game. It is quicker than a
release build and starts neoOMSI when the build finishes:

* **Windows:** `scripts\dev-windows.cmd`
* **macOS:** `sh scripts/dev-macos.sh`

You can add normal game arguments after either command.

On Windows, `scripts\dev-windows-release.cmd` makes a local build that is closer to a release
build. It keeps the normal release optimisation, but skips debug symbols and the slow final
optimisation pass. Use it when a bug only appears in a release-style build. It writes its files
to `dist\windows-dev` and is quicker than the proper release build, but it is not a substitute
for it.

Before opening a pull request, run the full checks:

* `cargo test --workspace`
* `cargo build --release`

To make a release build you can run locally, use `scripts\build-windows.cmd` on Windows or
`sh scripts/build-macos.sh` on macOS. CI also checks the supported platforms.

## Where things are

See the layout in the [README](README.md#repository-layout) and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). File formats: [docs/FORMATS.md](docs/FORMATS.md).

## Releases

Maintainers bump `MAJOR.MINOR` in the `VERSION` file; everything else is automatic - see
[docs/VERSIONING.md](docs/VERSIONING.md).
