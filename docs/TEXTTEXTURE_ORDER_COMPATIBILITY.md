# Text-texture order compatibility: Studio Polygon Gold VIX

## Status

**Compatibility workaround.** The parser normally uses declaration order, as OMSI
does. A guarded fallback handles models with missing declarations and references
that still use the old numbering. In-game verification is still required.

## Affected content

- Vehicle: `[SP] Studio Polygon 400MMC/goldn250.bus`
- Model file: `Model/E400MMC_Scania_GoldSpec.cfg`
- Feature: VIX TP5800 ticket machine, with the Scania dashboard, DG3 and CCTV
  text textures also present.

The unmodified Gold bus works in OMSI 2. In neoOMSI, its dashboard display
initially renders as white blocks. Moving only the VIX declarations can fix the
dashboard while causing VIX to show DG3/CCTV text. This is a useful minimal
reproduction because it proves that the wrong *live text textures* are being
bound, not that a source image or the VIX script is missing.

## Reproduction

1. Restore the original `E400MMC_Scania_GoldSpec.cfg`.
2. Start neoOMSI and load `goldn250.bus`.
3. Observe white blocks/incorrect regions on the Scania instrument display.
4. Move the 44 `VixTP5800_TT_*` `[texttexture_enh]` declarations to the end of
   the file without changing the VIX mesh references.
5. The normal dashboard improves, but the VIX screen displays text belonging to
   DG3, CCTV and other later definitions.

OMSI 2 renders the untouched original configuration correctly.

## Observed model layout

In the original Gold config:

- One text texture precedes the VIX block.
- The VIX block contains 44 `VixTP5800_TT_*` texture declarations.
- A further 30 normal dashboard/DG3/CCTV/etc. declarations follow it.
- The VIX mesh uses `[useTextTexture]` values `1` through `44`.

The labels preceding declarations are free text, not authoritative slot IDs.
Some are repeated or annotated in other models, so using them unconditionally
can overwrite working textures. This Gold configuration has fewer declarations
than the dashboard's `149–154` references, but its labels and references provide
enough consistent evidence to recover the missing slots as a compatibility measure.

Relevant implementation points to inspect:

- `crates/model/src/lib.rs` — parsing of `[texttexture]` and
  `[texttexture_enh]` declarations.
- `crates/core/src/scene/vehicle_instance.rs` — conversion of
  `use_text_texture` into a texture lookup.
- `simulation/src/texttex.rs` — text-texture creation and update lifecycle.

## Previous local workaround

The user installation has an automatically saved original at:

`C:\Program Files (x86)\Steam\steamapps\common\OMSI 2\Vehicles\[SP] Studio Polygon 400MMC\Model\E400MMC_Scania_GoldSpec.cfg.neoOMSI-original`

An earlier local experiment changed `E400MMC_Scania_GoldSpec.cfg` as follows:

1. Preserve all 30 non-VIX text texture definitions in their original sequence.
2. Append the 44 VIX declarations after them.
3. Change only the VIX mesh's `[useTextTexture]` values from `1..44` to
   `30..73`.

This was a content-side experiment, not a fix to ship. The active configuration
and the saved original matched when checked on 19 October 2026.

## Desired neoOMSI fix

Use declaration order by default. Only recover a sparse table when annotations
are monotonic, all material references resolve, at least two out-of-range
references match numbered annotations, and no previously valid reference changes.
This is a heuristic for malformed or modified content, not a claim that OMSI
interprets those labels. Ambiguous cases remain in declaration order.

## Acceptance tests

- Add a compact fixture based on the Gold structure: one pre-VIX texture, 44
  VIX textures, then normal DG3/CCTV textures.
- Assert that each VIX material reference resolves to the corresponding
  `VixTP5800_TT_*` target.
- Assert that dashboard/DG3/CCTV material references still resolve to their
  original targets.
- Render or snapshot-test both the dashboard and VIX surfaces where practical.
- Verify the installed Gold bus and an affected IVU/EN92 bus in-game.

## Regression risk

Changing this lookup can affect any bus using multiple text-texture groups.
Keep the fixture focused on order-sensitive mixed groups and test at least one
standard bus whose text textures already render correctly.
