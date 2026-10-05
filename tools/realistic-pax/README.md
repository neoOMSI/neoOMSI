# RealisticPax

Builds a content pack of realistic passengers: every adult and child of the
[Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) avatars (MIT) and the
professions whose work clothes a bus passenger might wear, plus children, teenagers and old
people made with MakeHuman's [MPFB](https://static.makehumancommunity.org/mpfb/) from its CC0
system assets (Rocketbox has four children and nobody old).

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
`build.py --hums-only` updates the `.hum` files without Blender. Copy the output to
`<content folder>/Packs/RealisticPax` and select **Settings → Gameplay → Passenger models →
Realistic** in the launcher. The pack is optional and applies on the next start.
Without it, installed OMSI passenger models remain in use.

## Licences

Nothing of the avatars is in the neoOMSI repository or its releases: `fetch.py` downloads
them and the pack is built on your machine.

| Source | Licence | In the pack |
| --- | --- | --- |
| [Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) avatars | MIT | `rocketbox/`, `LICENSE-Rocketbox.md` |
| MakeHuman base mesh, targets and system assets ([MakeHuman](http://www.makehumancommunity.org)) | CC0 1.0 | `generated/`, `LICENSE-MakeHuman.txt` |
| [MPFB](https://static.makehumancommunity.org/mpfb/) Blender add-on | GPL-3.0-or-later | nothing: it only runs in Blender while building |
| Your OMSI 2 installation's passenger `.hum` files | OMSI 2's | every `.hum`, with body, age and voice changed |

Because the `.hum` files are derived from OMSI 2's, a built pack is for your own use and must
not be redistributed. The scripts in this folder are part of neoOMSI and GPL-3.0-or-later like
the rest of its source.
