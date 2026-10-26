# Releasing & versioning

neoOMSI maintains a clear distinction between rapid development snapshots and stabilized community releases. There are no permanent `develop` or `nightly` branches; all active development integrates into `main`.

## Versioning scheme

neoOMSI follows Semantic Versioning (`MAJOR.MINOR.PATCH`) during pre-1.0 development:

| Type                  | Format                          | Example                    |
| --------------------- | ------------------------------- | -------------------------- |
| **Nightly**           | `0.x.y-nightly.g<engine-sha>`   | `0.2.0-nightly.gc3652dc0`  |
| **Release Candidate** | `v0.x.y-rc.<n>`                 | `v0.2.0-rc.1`              |
| **Stable**            | `v0.x.y`                        | `v0.2.0`                   |
| **Patch**             | `v0.x.y+1`                      | `v0.2.2`                   |

`1.0.0` is reserved for achieving comprehensive behavioral parity across the OMSI 2.2.032 baseline, not simply for elapsed development time.

In **Game & Updates**, players can select Stable, RC, or EA (Nightly). Each channel checks
its own GitHub releases; Stable is the default. EA and RC selections show a warning, and
installing an older release requires confirmation. **Check now** checks the selected channel.

## Build identity

Release CI stamps `neoomsi_BUILD_CHANNEL` as `stable`, `rc`, or `nightly` alongside
`neoomsi_VERSION`. Builds without an explicit channel are `developer`, including
local optimized builds.

## Build manifest

Every release publishes a machine-readable `build-manifest.json` asset recording the exact engine commit, version, channel, and CI run identity. When verifying existing immutable releases, CI validates the recorded manifest contents.

## Tags

Milestone git tags are created strictly for official releases:

```text
v0.2.0-rc.1
v0.2.0-rc.2
v0.2.0
v0.2.1
v0.2.2
```

Nightly snapshots publish under versioned tags (e.g. `v0.2.0-nightly.g<engine>`) on GitHub Releases.

## Release workflow

```text
main (nightlies)
  └── release/x.y.x → prepare → RC(s) → stable → sync to main
                         └── patch fixes → prepare → patch tag → sync to main
                            (repeat as needed; no RCs for patches)
```

### 1. Nightly builds

Automated CI builds run nightly at 03:37 in `Europe/Berlin` (and on manual workflow dispatch) for Windows, macOS, and Linux. Nightlies provide immediate visibility into recent changes but carry no guarantee against regressions.

The public Nightly version uses the source commit (`0.x.y-nightly.g<short-sha>`). The Actions run number remains linked in the release notes for CI traceability. Each nightly release shows changelog fragments changed since the previous release, falling back to all pending fragments when none are found.

Engine source tags live in GitLab; public release tags live in the Releases repo on GitHub and point to that repo's metadata commit. Before pushing any public RC or stable tag, sync the compiled `CHANGELOG.md` to the Releases repo. The public tag must contain that version's changelog section, and the matching GitLab source tag must already exist for GitHub Actions to build it.

For every release, a failed, skipped, or incomplete required GitHub Actions job blocks publication, including quota or infrastructure failures. Resolve the cause and rerun validation before announcing the release.

### 2. Minor release: freeze → prepare → RC → stable → sync

1. **Freeze:** Branch `release/x.y.x` from a tested `main` commit (e.g. `release/0.2.x`). Allow only release blockers, documentation corrections, and packaging fixes on the release branch; development continues on `main`.
2. **Prepare:** Update `VERSION`, run `bash scripts/compile-changelog.sh <version> <YYYY-MM-DD>`, review the resulting changelog and fragment removals, then commit as `chore(release): prepare <version>`.
3. **RC:** Check GitHub Actions capacity, tag `v<version>-rc.1` on the prepared source commit in GitLab, then sync the changelog and push the matching public RC tag. Verify the GitHub Actions release workflow, required platform artifacts, build manifest, and behavior across supported systems and representative content. Fix blockers on the release branch and issue `-rc.2`, `-rc.3`, etc. as needed.
4. **Stable:** When an RC has no known release blockers, tag **the exact accepted source commit** as `v<version>` in GitLab. Do not add an untested finalization commit.
5. **Sync:** Merge the release branch directly into source `main` without duplicating changelog entries. Sync the compiled `CHANGELOG.md` to the public Releases repo's `main`, then push its public `v<version>` tag on that changelog commit.
6. **Verify and announce:** Wait for the GitHub Actions workflow to finish with all required platform artifacts and the build manifest. Announce the stable release only after the workflow succeeds.

### 3. Patch release: fix → prepare → patch → sync

Patch releases **do not use Release Candidates**. On the current supported `release/x.y.x` branch:

1. Apply and review the necessary fixes; retain the release branch's feature freeze.
2. Update `VERSION`, compile and review the patch changelog with the same script, and commit as `chore(release): prepare <version>`.
3. Check GitHub Actions capacity and validate the exact source commit. Tag `v<version>` directly (e.g. `v0.2.3`) in GitLab, without an `-rc` tag.
4. Merge the release branch directly into source `main` without duplicating changelog entries. Sync the compiled `CHANGELOG.md` to the public Releases repo, then push the public `v<version>` tag on that changelog commit.
5. Verify the GitHub Actions release workflow, required platform artifacts, build manifest, and release notes before announcing the patch.
6. Repeat this patch cycle only when necessary.

### Early live support and support cutoff

Each stable minor line has a bounded **early live support** period after its initial release. Maintainers announce the planned final patch cycle or support cutoff for that line. During this period, reviewed maintenance fixes can be included in the planned patch releases; new features and routine parity or performance work continue on `main` for the next minor line.

After the announced cutoff, routine maintenance on that release line ends. Only an exceptional **release blocker** may be backported: a reproducible crash or data loss, a defect that prevents players from starting, loading, or continuing a normal session, or an issue that makes a supported multiplayer setup unusable, with no practical workaround. Every backport still requires maintainer review and the release validation above.

For every release, never move or reuse a published tag. Use ordinary Conventional Commits for fixes (e.g. `fix(graphics): recover from swapchain failure`); the sole standardized release-preparation message is `chore(release): prepare <version>`.

Only the latest stable minor line is normally maintained. Its `release/x.y.x` branch remains available through its announced early-live-support period. After the cutoff, the branch may be deleted; tags and published releases remain permanent, and a maintenance branch can be recreated from a tag for an exceptional release-blocker backport.

## Changelog management

To prevent merge conflicts across concurrent pull requests, contributors add small fragment files under `.changes/` instead of editing `CHANGELOG.md` directly:

```text
.changes/new.<category>.md
```

- **Merge Requests:** Fragments created as `new.<category>.md` are automatically renamed to `<mr-number>.<category>.md` by the CI pipeline upon opening.
- **Nightly CI:** Groups fragments changed since the previous rolling nightly by category and references each MR.
- **Stable Releases:** `scripts/compile-changelog.sh` uses the same renderer to compile all accumulated fragments into a new section in `CHANGELOG.md` and deletes the processed fragment files.
- **Fragment Guide:** See [.changes/README.md](../.changes/README.md) for naming rules, categories, and fragment formatting.
