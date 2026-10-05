# Contributing to neoOMSI

Thanks for your interest in contributing. neoOMSI optimizes for **correctness, simplicity, and long-term maintainability** over raw merge volume.

## Non-negotiable principles

1. **Clean-room implementation:** Never decompile proprietary OMSI binaries, commit proprietary source code, or import copyrighted assets (textures, meshes, audio, maps, scripts). All code must be an independent, clean-room implementation.
2. **Compatibility first:** If a change affects simulation behavior, verify what OMSI 2.2.032 actually does before altering engine logic. See [Compatibility policy](docs/COMPATIBILITY.md).
3. **No AI slop / Strict code ownership:** AI tools may assist your workflow, but generated code receives **no lower review standard**. Every line submitted must be understood, verified, and defended by the author. Speculative abstractions, unreviewed AI dumps, and vibe-coded refactors will be closed during triage.
4. **Focused PR scope:** Keep diffs tight and focused on a single architectural or behavioral boundary. Never combine bug fixes with unrelated cosmetic refactoring, reformatting, or dependency updates.

## Quick contribution checklist

1. **Build & test locally:**
   - Install prerequisites and use the fast local build scripts described in [Building from source](docs/BUILDING.md).
   - Ensure the workspace checks pass: `cargo nextest run --workspace`.
2. **Branch from `main`:**
   - Use short-lived, single-purpose branches (`feat/`, `fix/`, `parity/`, `refactor/`, `perf/`, `docs/`, `chore/`).
   - Details: [Development workflow](docs/DEVELOPMENT.md).
3. **Add a changelog fragment:**
   - User-visible PRs must include a small fragment under `.changes/` (e.g. `.changes/<pr-number>.<category>.md`).
   - Details: [.changes/README.md](.changes/README.md).
4. **Open your Pull Request:**
   - Document the _what_, _why_, and how you validated the change.
   - For `parity/` changes, provide reference evidence against OMSI 2.2.032.

## Deep-dive guides

| Topic                      | Document                                       |
| :------------------------- | :--------------------------------------------- |
| **Development & Review**   | [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)     |
| **OMSI 2 Compatibility**   | [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) |
| **Building & Testing**     | [docs/BUILDING.md](docs/BUILDING.md)           |
| **Issue Triage**           | [docs/ISSUE_TRIAGE.md](docs/ISSUE_TRIAGE.md)   |
| **Releasing & Versioning** | [docs/RELEASING.md](docs/RELEASING.md)         |
