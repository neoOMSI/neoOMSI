# OMSI 2 compatibility

Compatibility with **OMSI 2.2.032** is the primary behavioral target of neoOMSI.

The objective is to reproduce the behavior observable by OMSI content creators and players using a modern, maintainable Rust implementation.

## Reference baseline

OMSI 2.2.032 serves as the ground truth for legacy subsystems unless an intentional neoOMSI extension is explicitly documented:

- Input handling, key bindings, and force feedback
- Script execution, system variables, macros, and math semantics
- Vehicle physics, transmissions, pneumatic systems, and electrical circuits
- Map structures, splines, terrain, and scenery objects
- Pedestrian pathfinding, queues, and passenger ticketing
- AI traffic rules, priorities, and intersection control
- Timetables, duty schedules, HOF files, and IBIS matrix displays
- Dynamic weather, time progression, and seasonal variations
- Material definitions, lighting calculations, and camera projections
- Configuration file parsing and fallback mechanics

## Verification workflow

Parity changes must follow this sequence:

1. **Observe.** Isolate and reproduce the behavior in OMSI 2.2.032 under controlled conditions.
2. **Document.** Record test parameters: exact content, configuration, and boundary conditions.
3. **Compare.** Run the corresponding scenario in neoOMSI and capture discrepancies.
4. **Identify the root cause.** Determine the underlying subsystem rule rather than patching isolated visual or kinematic symptoms.
5. **Implement.** Apply the simplest correct, idiomatic fix to the core engine.
6. **Protect.** Add a deterministic regression test or fixture where feasible.

## Black-box methodology

Verification relies on black-box observation: inputs, outputs, configuration values, script state, event timing, and visible simulation state.

Contributors must **never** decompile original OMSI binaries, commit proprietary source code, or import copyrighted assets. All neoOMSI code must remain an independent, clean-room implementation.

## Parity evidence

A well-formed parity report or pull request includes:

- OMSI 2 version (defaults to 2.2.032)
- Map, vehicle, or scenario used for reproduction
- Minimal configuration or script snippets demonstrating the case
- Step-by-step reproduction instructions
- Expected behavior (OMSI 2) vs. observed behavior (neoOMSI)
- Supporting evidence: logs, captured variable states, or video where helpful

Subjective impressions ("steering feels different") must be broken down into measurable physical or kinematic parameters before changes are accepted.

## Compatibility status

Subsystem parity progresses through four explicit states:

```text
Unknown
  ↓
OMSI verified      (behavior characterized in OMSI 2.2.032)
  ↓
neoOMSI compared   (differences mapped to engine subsystems)
  ↓
Parity achieved    (verified and regression-tested)
```

Existing code that runs without crashes does not imply parity. Code remains provisional until validated against the reference baseline.

## Mod-specific issues

When an issue appears only with a particular third-party mod:

> Does this content expose a general OMSI rule that neoOMSI implements incorrectly?

- **Yes:** Correct the underlying engine rule. Use the mod as a test case.
- **No:** If the mod relies on undefined behavior, broken scripts, or non-standard configurations that even OMSI 2 does not guarantee, the issue may be documented and deferred or closed.

Do not introduce special-case workarounds or hardcode mod names into engine logic.

## Extensions

Features that extend beyond OMSI 2 are allowed but secondary to compatibility:

- Must be clearly identified as neoOMSI-specific.
- Must not alter default OMSI 2 behavior or break existing content.
- Must not force compatibility layers to guess whether legacy or extended semantics apply.
- Should be deferred if they add complexity to an unverified subsystem.

A configuration toggle is not an acceptable alternative to fixing broken baseline behavior.
