# Development workflow

This guide covers engineering standards, branching, code review, and development practices for neoOMSI.

## Engineering principles

When making trade-offs, prioritize:

1. **Correctness** – simulation behavior must accurately reflect verified reference behavior.
2. **Simplicity** – choose the simplest complete solution over complex abstractions.
3. **Maintainability** – clear data flow and explicit ownership beat clever or hidden mechanisms.
4. **Consistency** – follow established crate conventions and standard Rust idioms.
5. **Minimal churn** – keep diffs focused on the task at hand. Avoid formatting or refactoring untouched code.

## AI-assisted code standards

AI tools may assist with development, but generated code receives **no lower review standard**:

- Every line of submitted code must be understood, verified, and defended by the PR author.
- Speculative AI code dumps, unreviewed vibe-coded refactors, and phantom abstractions will be rejected during triage.
- Treat generated code as an untrusted draft: eliminate boilerplate, verify assumptions against OMSI 2.2.032 reference behavior, and write focused regression tests.

## Branching model

neoOMSI uses a **trunk-based workflow**. `main` is the sole permanent integration branch.

```text
main
├── feat/...         (new engine capabilities)
├── fix/...          (bug fixes and regressions)
├── parity/...       (verified compatibility improvements)
├── refactor/...     (simplifications preserving behavior)
├── perf/...         (performance optimizations)
├── docs/...         (documentation updates)
├── chore/...        (tooling and build infrastructure)
└── release/x.y      (release stabilization and current stable line)
```

### Working with branches

- Create focused, short-lived branches directly from `main`.
- Keep changes scoped to a single purpose. Avoid combining refactors with behavioral fixes.
- Delete branches upon merge.

### Release branches

`release/x.y` branches are created to stabilize Release Candidates and are retained for the currently supported stable line so patch releases can be cut without disturbing `main`. Once the next stable minor release is published, the previous release branch may be deleted. See [Releasing & versioning](RELEASING.md).

## Local development and testing

For fast iteration while developing, use the development scripts detailed in [Building from source](BUILDING.md):

- **Windows:** `scripts\dev-windows.cmd` (or `scripts\dev-windows-release.cmd` for optimized testing)
- **macOS:** `sh scripts/dev-macos.sh`

Before opening a pull request, verify that workspace checks pass:

```sh
cargo nextest run --workspace
cargo build --release
```

## Pull requests

### PR scope

- Keep pull requests small and focused on a single architectural or behavioral boundary.
- Do not mix parity fixes with unrelated cosmetic refactoring, formatting, or dependency updates.
- If a larger architectural defect is discovered, address only what is strictly necessary for the current fix and file a dedicated follow-up issue for the remainder.

### Review standards

Pull requests merged into `main` require:

- At least **two approving reviews** from maintainers.
- Passing continuous integration checks.
- For `parity/` changes: reference OMSI 2.2.032 evidence verifying expected behavior (see [Compatibility](COMPATIBILITY.md)).
- A changelog fragment for any user-visible change (see [.changes/README.md](../.changes/README.md)).

Once the required reviews and checks have passed, the PR author should normally perform the merge. Reviewers should leave the final merge to the author unless the author asks them to merge, the author is unavailable, or a maintainer has a clear operational reason to merge directly.

## Testing guidelines

- Bug fixes should include a unit or integration test reproducing the original issue whenever practical.
- Parsers, format deserializers, and math routines must have direct unit test coverage.
- Tests must **never** bundle proprietary OMSI 2 game assets. Where test fixtures are required, use synthetic mock data or optionally read from `OMSI_ROOT` (see [Building from source](BUILDING.md#running-tests)).

## Rain refraction snapshot

Rain films sample the current resolved scene after puddle reflections and before the
films are drawn. The separate snapshot avoids reading from the active colour attachment.
It uses half the scene width and height (rounded up, at least one texel), with a bilinear
downsample that preserves the scene's colour values without tonemapping or bloom filtering.
The scattering offsets are scaled to preserve their radius in scene pixels.
The downsample bind group is cached for the current source texture view and recreated
only when that view changes, including after target recreation or a reflection toggle.

The renderer requests `RG11B10UFLOAT_RENDERABLE` when supported and stores the snapshot
in `Rg11b10Ufloat` (4 bytes per texel); only RGB is used for refraction. Devices without
that feature use `Rgba16Float` (8 bytes per texel) at the same half resolution. At 1920×1080,
the compact snapshot's texture payload is approximately 1.98 MiB, compared with 15.82 MiB
for a full-resolution `Rgba16Float` snapshot. These sizes exclude allocation overhead and
do not measure total process VRAM usage.
