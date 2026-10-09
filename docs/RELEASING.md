# Releasing & versioning

neoOMSI maintains a clear distinction between rapid development snapshots and stabilized community releases. There are no permanent `develop` or `nightly` branches; all active development integrates into `main`.

## Versioning scheme

neoOMSI follows Semantic Versioning (`MAJOR.MINOR.PATCH`) during pre-1.0 development:

| Type                  | Format                       | Example                   |
| --------------------- | ---------------------------- | ------------------------- |
| **Nightly**           | `0.x.y-nightly.g<short-sha>` | `0.2.0-nightly.gdb76899d` |
| **Release Candidate** | `v0.x.y-rc.<n>`              | `v0.2.0-rc.1`             |
| **Stable**            | `v0.x.y`                     | `v0.2.0`                  |
| **Patch**             | `v0.x.y+1`                   | `v0.2.1`                  |

`1.0.0` is reserved for achieving comprehensive behavioral parity across the OMSI 2.2.032 baseline, not simply for elapsed development time.

## Build identity

Release CI stamps `neoomsi_BUILD_CHANNEL` as `stable`, `rc`, or `nightly` alongside
`neoomsi_VERSION`. Builds without an explicit channel are `developer`, including
local optimized builds.

## Launcher version

Every build packs the launcher at the full commit SHA in `scripts/launcher-ref`, a commit of
[neoOMSI/launcher](https://github.com/neoOMSI/launcher)'s `main`; the release notes link it.
Moving to a newer launcher is a PR of its own that changes that line, so a release can be
rebuilt as it was. Launcher and engine agree on the protocol version at their handshake
(`docs/LAUNCHER_PROTOCOL.md`).

## Tags

Milestone git tags are created strictly for official releases:

```text
v0.2.0-rc.1
v0.2.0-rc.2
v0.2.0
v0.2.1
```

Nightly snapshots publish under versioned tags (e.g. `v0.2.0-nightly.g<sha>`) on GitHub Releases.

## Release workflow

```text
main branch (trunk)
    │
    ├── Nightly builds (03:37 Europe/Berlin & manual dispatch)
    │
    └── Create release branch: release/0.2
            │
            ├── Tag: v0.2.0-rc.1 (testing)
            ├── Tag: v0.2.0-rc.2 (blocker fixes)
            │
            └── Tag: v0.2.0      (final release commit)
```

### 1. Nightly builds

Automated CI builds run nightly at 03:37 in `Europe/Berlin` (and on manual workflow dispatch) for Windows, macOS, and Linux. Nightlies provide immediate visibility into recent changes but carry no guarantee against regressions. Android packages are built locally using `scripts/build-android.sh`.

The public Nightly version uses the source commit (`0.x.y-nightly.g<short-sha>`). The Actions run number remains linked in the release notes for CI traceability. Each nightly release shows changelog fragments changed since the previous release, falling back to all pending fragments when none are found.

### 2. Preparing a Stable release

When `main` reaches a stabilization milestone:

1. Create a dedicated branch: `release/x.y`.
2. Enter **feature freeze**: only critical bug fixes, documentation corrections, and packaging fixes may be committed to this branch.
3. Keep `main` open for ongoing feature and parity development. Ensure all fixes on the release branch are cherry-picked back into `main`.

### 3. Release Candidates (RCs)

1. Tag the first candidate from the release branch (e.g. `v0.4.0-rc.1`).
2. Conduct testing across supported operating systems, maps, buses, and hardware configurations.
3. If release blockers are discovered, apply the fix to the release branch and tag `v0.4.0-rc.2`.

### 4. Tagging the Stable release

Once an RC exhibits no known release blockers:

- Tag the **exact commit** of the final accepted RC as the Stable release (e.g. `v0.4.0`).
- Avoid pushing last-minute, unvalidated changes between the final RC and the release tag.

### 5. Patch releases

If critical issues are identified post-release:

- Fixes are applied directly to the corresponding `release/x.y` branch.
- Tag and publish a patch release (e.g. `v0.4.1`).
- Mirror the fix into `main`.

Only the latest stable minor line is actively maintained. Its `release/x.y` branch remains available for patches until the next stable minor release is published. Older release branches may then be deleted; their tags and published releases remain permanent, and a maintenance branch can be recreated from a tag if an exceptional backport is ever required.

## Changelog management

To prevent merge conflicts across concurrent pull requests, contributors add small fragment files under `.changes/` instead of editing `CHANGELOG.md` directly:

```text
.changes/<pr-number>.<category>.md
```

- **Nightly CI:** Groups fragments changed since the previous rolling nightly by category, links each entry to its PR, and links packaged downloads directly.
- **Stable Releases:** `scripts/compile-changelog.sh` uses the same renderer to compile all accumulated fragments into a new section in `CHANGELOG.md` and deletes the processed fragment files.
- **Fragment Guide:** See [.changes/README.md](../.changes/README.md) for naming rules, categories, and fragment formatting.
