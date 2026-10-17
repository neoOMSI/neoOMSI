# Text-texture order compatibility: Studio Polygon Gold VIX

## Status

**Open neoOMSI compatibility issue.** A local, bus-side workaround is installed for
testing only. It must be removable once neoOMSI resolves text textures with the
same semantics as OMSI 2.

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

Each text-texture declaration is preceded by its explicit numeric slot. neoOMSI
was discarding that slot, packing declarations into encounter order, then using
`[useTextTexture]` as a direct position in the packed collection. The Gold
configuration has sparse slots, so dashboard materials that request slots
`149–154` received no matching generated texture.

Relevant implementation points to inspect:

- `crates/model/src/lib.rs` — parsing of `[texttexture]` and
  `[texttexture_enh]` declarations.
- `crates/core/src/scene/vehicle_instance.rs` — conversion of
  `use_text_texture` into a texture lookup.
- `simulation/src/texttex.rs` — text-texture creation and update lifecycle.

## Temporary local workaround currently applied

The user installation has an automatically saved original at:

`C:\Program Files (x86)\Steam\steamapps\common\OMSI 2\Vehicles\[SP] Studio Polygon 400MMC\Model\E400MMC_Scania_GoldSpec.cfg.neoOMSI-original`

The active `E400MMC_Scania_GoldSpec.cfg` has been changed locally as follows:

1. Preserve all 30 non-VIX text texture definitions in their original sequence.
2. Append the 44 VIX declarations after them.
3. Change only the VIX mesh's `[useTextTexture]` values from `1..44` to
   `30..73`.

This leaves the dashboard definitions at positions `0..29`, and puts the VIX
definitions at `30..73`; it prevents the VIX mesh from consuming DG3/CCTV
slots. It is intentionally **not** a content fix to ship.

## Desired neoOMSI fix

When parsing `[texttexture]` and `[texttexture_enh]`, preserve the preceding
numeric slot and populate the generated-texture table at that slot, retaining
holes. Do not require vehicle authors to reorder blocks or renumber
`[useTextTexture]` values.

## Acceptance tests

- Add a compact fixture based on the Gold structure: one pre-VIX texture, 44
  VIX textures, then normal DG3/CCTV textures.
- Assert that each VIX material reference resolves to the corresponding
  `VixTP5800_TT_*` target.
- Assert that dashboard/DG3/CCTV material references still resolve to their
  original targets.
- Render or snapshot-test both the dashboard and VIX surfaces where practical.
- Verify the original, unmodified `goldn250.bus` model works, then restore the
  local config from its `.neoOMSI-original` backup.

## Regression risk

Changing this lookup can affect any bus using multiple text-texture groups.
Keep the fixture focused on order-sensitive mixed groups and test at least one
standard bus whose text textures already render correctly.
