# Content validation

`omsi-check` checks OMSI maps, vehicles and their dependencies for missing files,
invalid values and script errors. It does not change your content or start OMSI.

## Usage

Run these commands from the repository root. Replace the installation path as needed.

```powershell
$omsi = 'F:\SteamLibrary\steamapps\common\OMSI 2'

# Check the whole installation
cargo run -p omsi-check -- $omsi

# Check one map and its dependencies
cargo run -p omsi-check -- $omsi --map maps/Grundorf/global.cfg

# Check one vehicle and save a JSON report
cargo run -p omsi-check -- $omsi --vehicle Vehicles/MAN_SD202/MAN_D86.bus --format json --output report.json
```

Repeat `--map`, `--vehicle` or `--package` to check several files or folders.
Paths can be absolute or relative to the installation. Use `--content PATH` to
include an add-on folder or ZIP. These roots take priority over the installation,
in the order given.

Useful options:

- `--only maps` limits the initial search to maps. Dependencies are still checked.
- `--target omsi2|neoomsi|both` selects which compatibility findings to show. The default is `both`.
- `--fail-on warning` also fails the check on warnings. By default, only errors cause failure.
- `--verbose` shows all findings. Text reports show up to 100 by default; JSON includes everything.
- `--run` adds short startup checks using neoOMSI. Each object has a 60-second timeout, adjustable with `--timeout`.

Use `cargo run -p omsi-check -- --help` for all options.

## Results and limits

Exit code **0** means the checks finished without reaching the failure threshold.
**1** means findings reached that threshold. **2** means the invocation was invalid
or the checks could not finish. Review the report for files that were not checked.

A successful check does not guarantee that content will work in OMSI. The tool
cannot check every script-generated path, chronology combination or gameplay
situation. Startup checks use neoOMSI without rendering, traffic or passengers.
Findings marked `neoomsi` do not necessarily apply to the original OMSI.

The `content-validation` library provides the same checks through `ScanOptions`
and `scan()`. Run library scans in a dedicated process because the content loaders
share global state.
