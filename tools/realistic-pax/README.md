# RealisticPax

Builds a content pack of realistic passengers: every adult of the
[Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) avatars (MIT) and the
professions whose work clothes a bus passenger might wear, plus children, teenagers and old
people made with MakeHuman's [MPFB](https://static.makehumancommunity.org/mpfb/) from its CC0
system assets (Rocketbox has nobody old, and its children are its adults made smaller). The
generated people are made as tall as people of their age and sex are.

The pack is plain OMSI content (`.hum`, `.cfg`, `.o3d`, DDS textures) on the stock 13-bone
rig, with three LOD levels and three recoloured outfits (`[CTC]` clothing variants) per
figure. `pax.json` maps avatars to stock `.hum` slots, defines weighted alternates, and
describes generated MakeHuman figures, including age and walk parameters.

Needs Python 3 with Pillow 11+ and Blender 4.2+.

```bash
python tools/realistic-pax/fetch.py
python tools/realistic-pax/build.py --omsi "C:/Steam/steamapps/common/OMSI 2"
```

`fetch.py` downloads about 3.5 GB of source assets into `.cache/`; `build.py` writes to
`build/RealisticPax` or the directory passed to `--out`. After changing weights or voices,
`build.py --hums-only` updates the `.hum` files without Blender.

Players do not run any of this: the launcher downloads the pack (**Settings → Gameplay →
Download the realistic passengers**) from the release tagged `realistic-pax-v2`, checks it
against the SHA-256 GitHub lists and installs it into `<content folder>/Packs/RealisticPax`.
To publish a new pack:

1. Build it and run `python tools/realistic-pax/build.py --package`, which writes
   `build/RealisticPax-v<N>.zip` (with `RealisticPax/pack.json` saying version `N`).
2. Upload that file to a GitHub release tagged `realistic-pax-v<N>`, as a pre-release (not
   marked latest).
3. For a new version, raise `PACK_VERSION` in `build.py` and `TAG`, `FILE` and `VERSION` in
   `crates/omsi-app/src/pax_pack.rs`; the launcher then offers installed older packs an
   update. A pack copied in by hand has no `pack.json` and is left alone.

## Licences

Nothing of the avatars is in the neoOMSI repository or the program's releases: the pack is a
release file of its own that the launcher downloads on request.

| Source | Licence | In the pack |
| --- | --- | --- |
| [Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) avatars | MIT | `rocketbox/`, `LICENSE-Rocketbox.md` |
| MakeHuman base mesh, targets and system assets ([MakeHuman](http://www.makehumancommunity.org)) | CC0 1.0 | `generated/`, `LICENSE-MakeHuman.txt` |
| [MPFB](https://static.makehumancommunity.org/mpfb/) Blender add-on | GPL-3.0-or-later | nothing: it only runs in Blender while building |
| OMSI 2's passenger `.hum` files | OMSI 2's | nothing: `build.py` writes each `.hum` itself and takes only a few numbers of the one it replaces (seat height, step, voice, age) |

The scripts in this folder are part of neoOMSI and GPL-3.0-or-later like the rest of its source.
