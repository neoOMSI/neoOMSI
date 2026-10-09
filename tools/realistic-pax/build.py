"""python build.py --omsi "<OMSI 2 folder>" [--out <pack folder>] [--blender <blender.exe>] [--size 1024] [--jobs N]
[--only <slot .hum>] [--hums-only]
python build.py --package [--out <pack folder>]
"""

import argparse
import concurrent.futures
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import zipfile

from PIL import Image, ImageChops

HERE = pathlib.Path(__file__).resolve().parent
CACHE = HERE / ".cache"
MPFB_PROFILE = CACHE / "mpfb" / "profile"
# screen size (twice the person's height over the distance, in view heights) each
# level is drawn down to: about 14 m and 43 m at a 60 degree view
LOD_SIZES = [0.25, 0.08, 0.0]
VARIANTS = [(140, (0.35, 0.45, 0.8), 0.45, 1.0), (215, (0.75, 0.35, 0.3), 0.4, 1.0),
            (0, (0.5, 0.5, 0.5), 0.0, 0.45)]
BONES = ["OS_L", "OS_R", "US_L", "US_R", "OA_L", "OA_R", "UA_L", "UA_R",
         "Hip", "Main", "Head", "Hand_L", "Hand_R"]
# median height (m) of boys and girls by age (WHO), and of men and women
GROWTH = {6: (1.17, 1.16), 7: (1.24, 1.23), 8: (1.30, 1.29), 9: (1.35, 1.34), 10: (1.40, 1.39),
          11: (1.45, 1.45), 12: (1.51, 1.52), 13: (1.58, 1.57), 14: (1.65, 1.61),
          15: (1.71, 1.63), 16: (1.75, 1.64), 17: (1.77, 1.65)}
ADULT = (1.79, 1.66)


def stature(person):
    """The median of the age and sex, a few per cent either way by the height macro."""
    p = person["spec"]["phenotype"]
    female = int(p["gender"] < 0.5)
    age = person["age"]
    if age < 18:
        base = GROWTH[min(max(age, 6), 17)][female]
    else:
        base = ADULT[female] - 0.0015 * max(0, age - 50)
    return round(base * (1.0 + 0.12 * (p.get("height", 0.5) - 0.5)), 3)


def find_blender(given):
    if given:
        return given
    if os.environ.get("BLENDER"):
        return os.environ["BLENDER"]
    found = shutil.which("blender")
    if found:
        return found
    base = pathlib.Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Blender Foundation"
    for exe in sorted(base.glob("Blender */blender.exe"), reverse=True):
        return str(exe)
    sys.exit("Blender not found: pass --blender or set BLENDER")


def blocks(text):
    return [(i, line.strip().lower()) for i, line in enumerate(text.splitlines())
            if re.fullmatch(r"\[[^\]]+\]\s*", line)]


def values(lines, at, n):
    return [lines[at + 1 + k].strip() for k in range(n)]


def replace_values(lines, keyword, new):
    for i, k in blocks("\n".join(lines)):
        if k == keyword:
            for j, v in enumerate(new):
                lines[i + 1 + j] = v
            return
    lines.extend(["", keyword] + new)


def voice_for(female, age, stock):
    """The ticket packs' voices: F1-F4 and M1-M4 adults (M4 the youngest, M3 the deepest),
    FY1 and MY1 children, FO1 an old woman; there is no old man's."""
    if age <= 14:
        return "FY1" if female else "MY1"
    if age >= 60:
        return "FO1" if female else "M3"
    if age < 20 and not female:
        return "M4"
    if stock in ("MY1", "FY1", "FO1"):
        return "F2" if female else "M2"
    return stock


def figure_meta(stock_text, source, weight, person):
    """What the .hum says about the figure beyond its body: how often it comes up, and an age
    (tickets) and voice that fit it rather than the stock person whose place it takes."""
    lines = stock_text.splitlines()
    found = dict((k, i) for i, k in blocks(stock_text))
    stock_voice = values(lines, found["[voice]"], 1)[0] if "[voice]" in found else ""
    stock_age = int(values(lines, found["[age]"], 1)[0]) if "[age]" in found else None
    if person:
        female = person["spec"]["phenotype"]["gender"] < 0.5
        age = person["age"]
    else:
        female = "Female" in source
        age = stock_age if source.startswith("Children/") and stock_age else 35
    meta = [("[neo_weight]", [f"{weight:g}"])] if weight != 1.0 else []
    if age != stock_age and (person or stock_age is not None):
        meta.append(("[age]", [str(age)]))
    voice = voice_for(female, age, stock_voice)
    if stock_voice and voice != stock_voice:
        meta.append(("[voice]", [voice]))
    if person and person.get("walk"):
        meta.append(("[walk_param]", [f"{v:g}" for v in person["walk"]]))
    return meta


def write_hum(stock_path, out_path, model, height, links, meta):
    """A .hum of our own, so that the pack can be passed on: only the keywords the engine
    reads, with the stock person's few numbers where the figure takes its place."""
    text = stock_path.read_bytes().decode("cp1252")
    lines = text.splitlines()
    found = dict((k, i) for i, k in blocks(text))

    def stock(keyword, n):
        return values(lines, found[keyword], n) if keyword in found else []

    stock_links = [float(v) for v in stock("[links]", 22)] or [0.0] * 22
    seat = float((stock("[seatheight]", 1) or ["0"])[0])
    hum = {
        "[model]": [model],
        "[humangeom]": [(stock("[humangeom]", 1) or ["0.04"])[0], f"{height:.2f}"],
        "[links]": [f"{v:.3f}" for v in links],
        "[voice]": stock("[voice]", 1),
        "[walk_param]": stock("[walk_param]", 5),
        "[mass]": stock("[mass]", 1),
        "[age]": stock("[age]", 1),
    }
    if seat > 0.2:
        # the stock hip-above-seat lift: the new body sits as deep as the old one did
        hum["[seatheight]"] = [f"{links[2] - (stock_links[2] - seat):.2f}"]
    for keyword, new in meta:
        hum[keyword] = new
    out = [line for k, v in hum.items() if v for line in [k, *v, ""]]
    out_path.write_bytes(("\r\n".join(out) + "\r\n").encode("cp1252"))


def rewrite_hum(stock_path, out_path, source, weight, person):
    """A .hum built before written again from its body's numbers: no Blender needed."""
    text = out_path.read_bytes().decode("cp1252")
    lines = text.splitlines()
    found = dict((k, i) for i, k in blocks(text))
    model = values(lines, found["[model]"], 1)[0]
    height = float(values(lines, found["[humangeom]"], 2)[1])
    links = [float(v) for v in values(lines, found["[links]"], 22)]
    meta = figure_meta(stock_path.read_bytes().decode("cp1252"), source, weight, person)
    if "[seatheight]" in found:
        # (recomputed from the rounded links it could come out a centimetre off)
        meta.append(("[seatheight]", values(lines, found["[seatheight]"], 1)))
    write_hum(stock_path, out_path, model, height, links, meta)
    return f"{out_path.relative_to(out_path.parents[2])} <- {source}"


def write_cfg(path, meshes, textures, alpha, ctc):
    out = []
    if ctc:
        folder, body = ctc
        out += ["[CTC]", "Colorscheme", folder, "0", "", "[CTCTexture]", "farbschema", body, ""]
    for size, mesh in zip(LOD_SIZES, meshes):
        out += ["[LOD]", str(size), "", "[mesh]", mesh, ""]
        for b in BONES:
            out += ["[setbone]", b, str(-2 - BONES.index(b)), ""]
        for t in textures:
            out += ["[matl]", t, "0"]
            if t in alpha:
                out += ["[matl_alpha]", "1"]
            out.append("")
    path.write_bytes(("\r\n".join(out) + "\r\n").encode("cp1252"))


def write_dds(img, dst, pixel_format):
    """Block-compressed with the whole mip chain: the engine uploads such a file as it is."""
    block = 8 if pixel_format == "DXT1" else 16
    data, header, levels = [], None, 0
    while True:
        buf = io.BytesIO()
        img.save(buf, format="DDS", pixel_format=pixel_format)
        raw = buf.getvalue()
        header = header or bytearray(raw[:128])
        want = max(1, (img.width + 3) // 4) * max(1, (img.height + 3) // 4) * block
        if len(raw) - 128 != want:
            raise SystemExit(f"{dst}: unexpected DDS level size {len(raw) - 128}, want {want}")
        data.append(raw[128:])
        levels += 1
        if img.width == 1 and img.height == 1:
            break
        img = img.resize((max(1, img.width // 2), max(1, img.height // 2)), Image.LANCZOS)
    flags, = struct.unpack_from("<I", header, 8)
    struct.pack_into("<I", header, 8, flags | 0x20000)
    struct.pack_into("<I", header, 28, levels)
    caps, = struct.unpack_from("<I", header, 108)
    struct.pack_into("<I", header, 108, caps | 0x400008)
    dst.write_bytes(bytes(header) + b"".join(data))


def ramp(lo, hi):
    return [round(255 * min(1.0, max(0.0, (x - lo) / (hi - lo)))) for x in range(256)]


def skin_hue(x):
    deg = x / 255 * 360
    d = min(abs(deg - 22), 360 - abs(deg - 22))
    return round(255 * min(1.0, max(0.0, 1 - (d - 20) / 14)))


def recolor(rgb, hue_turn, tint, tint_amount, value):
    """The cloth in other colours; skin (orange hues, neither grey nor garish) stays."""
    h, s, v = rgb.convert("HSV").split()
    skin = ImageChops.multiply(h.point([skin_hue(x) for x in range(256)]),
                               s.point(ramp(0.1 * 255, 0.2 * 255)))
    skin = ImageChops.multiply(skin, s.point([255 - y for y in ramp(0.65 * 255, 0.8 * 255)]))
    skin = ImageChops.multiply(skin, v.point(ramp(0.08 * 255, 0.18 * 255)))
    coloured = s.point(ramp(0.12 * 255, 0.22 * 255))
    turn = round(hue_turn / 360 * 256)
    shifted = Image.merge("HSV", (h.point([(x + turn) % 256 for x in range(256)]), s, v))
    shifted = shifted.convert("RGB")
    lum = rgb.convert("L")
    tinted = Image.merge("RGB", [lum.point([min(255, round(x * c * 1.6)) for x in range(256)])
                                 for c in tint])
    out = Image.composite(shifted, Image.blend(rgb, tinted, tint_amount), coloured)
    if value != 1.0:
        out = out.point([round(x * value) for x in range(256)] * 3)
    return Image.composite(rgb, out, skin)


def has_alpha(img):
    return img.mode in ("RGBA", "LA", "PA", "P") and img.convert("RGBA").getchannel("A").getextrema()[0] < 250


def greyed(img):
    rgba = img.convert("RGBA")
    grey = rgba.convert("L").point([round(150 + x * 0.42) for x in range(256)])
    out = Image.merge("RGBA", (grey, grey, grey, rgba.getchannel("A")))
    return out if has_alpha(img) else out.convert("RGB")


def fit(img, size):
    if max(img.size) > size:
        k = size / max(img.size)
        img = img.resize((max(4, round(img.width * k)), max(4, round(img.height * k))), Image.LANCZOS)
    return img


def write_variants(src, folder, size):
    folder.mkdir(parents=True, exist_ok=True)
    img = fit(Image.open(src), size)
    alpha = img.convert("RGBA").getchannel("A") if has_alpha(img) else None
    rgb = img.convert("RGB")
    items = []
    for k, (turn, tint, amount, value) in enumerate(VARIANTS, 1):
        name = f"{src.stem}_v{k}.dds"
        if not (folder / name).exists():
            out = recolor(rgb, turn, tint, amount, value)
            if alpha:
                out.putalpha(alpha)
            write_dds(out, folder / name, "DXT5" if alpha else "DXT1")
        items += ["[item]", f"Variante{k}", "farbschema", name, ""]
    (folder / "texvarianten.cti").write_bytes(("\r\n".join(items) + "\r\n").encode("cp1252"))


def convert_texture(img, dst, size, keep_alpha):
    img = fit(img.convert("RGBA" if keep_alpha else "RGB"), size)
    write_dds(img, dst, "DXT5" if keep_alpha else "DXT1")


def weight_of(avatar, weights):
    best = ""
    for prefix in weights:
        if avatar.startswith(prefix) and len(prefix) > len(best):
            best = prefix
    return weights.get(best, 1.0)


def build_figure(args, blender, stock, hum_out, source, weight, person=None):
    env = None
    if person:
        name = person["name"]
        model_dir = hum_out.parent / "generated" / name
        model_dir.mkdir(parents=True, exist_ok=True)
        spec = model_dir / f"{name}.person.json"
        spec.write_text(json.dumps({**person["spec"], "neo_height": stature(person)}))
        given = str(spec)
        env = dict(os.environ)
        for var, sub_dir in (("BLENDER_USER_RESOURCES", ""), ("BLENDER_USER_CONFIG", "config"),
                             ("BLENDER_USER_SCRIPTS", "scripts"), ("BLENDER_USER_DATAFILES", "datafiles"),
                             ("BLENDER_USER_EXTENSIONS", "extensions")):
            env[var] = str(MPFB_PROFILE / sub_dir)
        files = {}
        model_rel = f"generated\\{name}"
    else:
        name = source.rsplit("/", 1)[1]
        model_dir = hum_out.parent / "rocketbox" / name
        given = str(CACHE / source / "Export" / f"{name}.fbx")
        files = {f.name.lower(): f for f in (CACHE / source / "Textures").iterdir()}
        model_rel = f"rocketbox\\{name}"
    tex_dir = model_dir / "texture"
    tex_dir.mkdir(parents=True, exist_ok=True)
    sidecar = model_dir / f"{name}.json"
    cmd = [blender, "-b", "-P", str(HERE / "blender_export.py"), "--", given,
           str(model_dir / name), str(sidecar)]
    if not person:
        cmd.insert(2, "--factory-startup")
    r = subprocess.run(cmd, capture_output=True, text=True, env=env)
    if r.returncode != 0 or not sidecar.exists():
        raise RuntimeError(f"{source}: Blender failed\n{r.stdout[-3000:]}\n{r.stderr[-3000:]}")
    info = json.loads(sidecar.read_text())
    sidecar.unlink()
    if person:
        spec.unlink()
    textures, alpha, sources = [], [], {}
    for m in info["materials"]:
        if not m["texture"]:
            continue
        path = pathlib.Path(m["texture"])
        if not path.exists():
            path = files.get(path.name.lower())
        if path is None:
            raise RuntimeError(f"{source}: no texture {m['texture']} for material {m['name']}")
        dds = path.stem + ".dds"
        if dds in textures:
            continue
        img = Image.open(path)
        if person and person.get("age", 0) >= 60 and re.search(r"[\\/](hair|eyebrows)[\\/]", str(path)):
            img = greyed(img)
        see_through = "opacity" in m["name"].lower() or "opacity" in path.stem.lower() or has_alpha(img)
        textures.append(dds)
        sources[dds] = path
        if see_through:
            alpha.append(dds)
        if not (tex_dir / dds).exists():
            convert_texture(img, tex_dir / dds, args.size, see_through)
    # the clothes recoloured: Rocketbox has them on the body's texture, MakeHuman a suit's
    body = next((t for t in textures if "_body_color" in t.lower()), None) \
        or next((t for t in textures if "suit" in t.lower()), None)
    ctc = None
    if body:
        write_variants(sources[body], model_dir / "variants", args.size)
        ctc = (f"{model_rel}\\variants", body)
    write_cfg(model_dir / f"{name}.cfg", [lv["file"] for lv in info["levels"]], textures, alpha, ctc)
    meta = figure_meta(stock.read_bytes().decode("cp1252"), source, weight, person)
    write_hum(stock, hum_out, f"{model_rel}\\{name}.cfg", info["height"], info["links"], meta)
    tris = " / ".join(str(lv["triangles"]) for lv in info["levels"])
    return f"{hum_out.relative_to(args.out)} <- {source}: {tris} triangles, {info['height']} m"


MAKEHUMAN_LICENSE = """\
The people under generated/ are made with MakeHuman's MPFB from MakeHuman's base mesh,
targets and system assets (skins, eyes, eyebrows, hair, clothes, shoes), which their
copyright holders released into the public domain under CC0 1.0 Universal:
https://creativecommons.org/publicdomain/zero/1.0/

The copyright holders at the point of the release to CC0 were:
Copyright (C) 2020 Data Collection AB, https://www.datacollection.se
Copyright (C) 2020 Joel Palmius
Copyright (C) 2020 Jonas Hauquier

MakeHuman: http://www.makehumancommunity.org
MPFB (GPL-3.0-or-later) only ran in Blender to make the models; none of it is in this pack.
"""

PACK_README = """\
Realistic passengers for neoOMSI, built with tools/realistic-pax of the neoOMSI sources.

- rocketbox/: the Microsoft Rocketbox avatars
  (https://github.com/microsoft/Microsoft-Rocketbox), MIT licence, see LICENSE-Rocketbox.md.
- generated/: people made with MakeHuman's MPFB from CC0 assets, see LICENSE-MakeHuman.txt.
- Each <name>.hum takes the place of the OMSI 2 passenger of the same name, the
  <name>~<other>.hum files are drawn in its place now and then. They hold nothing of
  OMSI 2's files but a few of their numbers (seat height, step, voice, age).

The neoOMSI launcher downloads the pack (Settings -> Gameplay); pack.json is its version.
"""

# what the launcher offers again when it is newer than the installed pack's (pax_pack.rs)
PACK_VERSION = 2


def write_pack_notes(out, generated):
    if (CACHE / "LICENSE.md").exists() or not (out / "LICENSE-Rocketbox.md").exists():
        shutil.copy(CACHE / "LICENSE.md", out / "LICENSE-Rocketbox.md")
    if generated:
        (out / "LICENSE-MakeHuman.txt").write_text(MAKEHUMAN_LICENSE)
    (out / "README.txt").write_text(PACK_README)
    (out / "pack.json").write_text(json.dumps({"name": "RealisticPax", "version": PACK_VERSION}))


def package(out):
    """The release file the launcher downloads: the pack under a RealisticPax folder."""
    zip_path = out.parent / f"RealisticPax-v{PACK_VERSION}.zip"
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for f in sorted(out.rglob("*")):
            if f.is_file():
                z.write(f, pathlib.PurePosixPath("RealisticPax", *f.relative_to(out).parts))
    digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
    print(f"{zip_path}: {zip_path.stat().st_size / 1e6:.0f} MB, sha256 {digest}")
    print(f"upload it as a file of the release tagged realistic-pax-v{PACK_VERSION}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--omsi", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path, default=HERE / "build" / "RealisticPax")
    ap.add_argument("--blender")
    ap.add_argument("--size", type=int, default=1024)
    ap.add_argument("--jobs", type=int, default=max(1, min(6, (os.cpu_count() or 2) // 2)))
    ap.add_argument("--only", help="build just this slot (a .hum as named in pax.json)")
    ap.add_argument("--hums-only", action="store_true",
                    help="rewrite only the weights, ages and voices of a pack built before")
    ap.add_argument("--package", action="store_true",
                    help="zip the pack built before for the release the launcher downloads")
    args = ap.parse_args()
    if args.package:
        package(args.out)
        return
    if not args.omsi:
        ap.error("--omsi is needed to build the pack")
    blender = None if args.hums_only else find_blender(args.blender)
    pax = json.loads((HERE / "pax.json").read_text())

    work = []
    people = {f"generated/{p['name']}": p for p in pax.get("generated", [])}
    for hum, avatar in pax["slots"].items():
        if args.only and hum != args.only:
            continue
        stock = args.omsi / hum
        if not stock.exists():
            print(f"skip {hum}: not in {args.omsi}")
            continue
        if avatar in people:
            person = people[avatar]
            work.append((stock, args.out / hum, avatar, person.get("weight", 1.0), person))
        else:
            work.append((stock, args.out / hum, avatar, weight_of(avatar, pax["weights"])))
        for alt in pax["alternates"].get(hum, []):
            out = args.out / hum
            out = out.with_name(f"{out.stem}~{alt.rsplit('/', 1)[1]}.hum")
            work.append((stock, out, alt, weight_of(alt, pax["weights"])))
    for person in pax.get("generated", []):
        hum = person["slot"]
        if (args.only and hum != args.only) or not (args.omsi / hum).exists():
            continue
        if pax["slots"].get(hum) == f"generated/{person['name']}":
            continue
        out = args.out / hum
        out = out.with_name(f"{out.stem}~{person['name']}.hum")
        work.append((args.omsi / hum, out, f"generated/{person['name']}",
                     person.get("weight", 1.0), person))

    if args.hums_only:
        for stock, out, source, weight, *person in work:
            print(rewrite_hum(stock, out, source, weight, person[0] if person else None))
        write_pack_notes(args.out, bool(pax.get("generated")))
        return
    failed = False
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        jobs = [pool.submit(build_figure, args, blender, *w) for w in work]
        for job in concurrent.futures.as_completed(jobs):
            try:
                print(job.result(), flush=True)
            except RuntimeError as e:
                print(e, flush=True)
                failed = True

    write_pack_notes(args.out, bool(pax.get("generated")))
    print(f"pack written to {args.out}")
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
