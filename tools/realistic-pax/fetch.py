"""Download what build.py makes the pack from: the Rocketbox avatars named in pax.json (mesh and
colour textures only), and MakeHuman's MPFB with its CC0 system assets, installed into a Blender
profile of its own under .cache/mpfb (the Blender you use is left as it is)."""

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import urllib.request
import zipfile

from build import CACHE, MPFB_PROFILE, find_blender

REPO = "microsoft/Microsoft-Rocketbox"
REVISION = "0943055db6ec570bcef9f2c8b41c9e5467c808f9"
HERE = pathlib.Path(__file__).resolve().parent
MPFB_SHA256 = "4f0a879d64a39bf646fbf5f53601ac678855da329d650617dca5737548239a87"
MPFB_URL = (f"https://extensions.blender.org/download/sha256:{MPFB_SHA256}/add-on-mpfb-v2.0.17.zip"
            "?repository=%2Fapi%2Fv1%2Fextensions%2F&blender_version_min=4.2.0")
ASSETS_URL = ("https://files.makehumancommunity.org/asset_packs/makehuman_system_assets/"
              "makehuman_system_assets_cc0.zip")


def wanted(path, avatar):
    name = avatar.rsplit("/", 1)[1]
    if path == f"Assets/Avatars/{avatar}/Export/{name}.fbx":
        return True
    return path.startswith(f"Assets/Avatars/{avatar}/Textures/") \
        and "_color" in path.rsplit("/", 1)[1] and path.endswith(".tga")


def download(url, out):
    out.parent.mkdir(parents=True, exist_ok=True)
    tmp = out.with_suffix(out.suffix + ".part")
    urllib.request.urlretrieve(url, tmp)
    tmp.replace(out)


def git_blob_hash(path):
    data = path.read_bytes()
    return hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()


def rocketbox(pax):
    avatars = sorted((set(pax["slots"].values()) | {a for v in pax["alternates"].values() for a in v})
                     - {f"generated/{p['name']}" for p in pax.get("generated", [])})
    with urllib.request.urlopen(
        f"https://api.github.com/repos/{REPO}/git/trees/{REVISION}?recursive=1"
    ) as r:
        tree = json.load(r)["tree"]
    for avatar in avatars:
        files = [t for t in tree if t["type"] == "blob" and wanted(t["path"], avatar)]
        if not any(f["path"].endswith(".fbx") for f in files):
            sys.exit(f"{avatar}: not in {REPO}")
        for f in files:
            out = CACHE / f["path"].removeprefix("Assets/Avatars/")
            if out.exists() and git_blob_hash(out) == f["sha"]:
                continue
            print(f"{f['path']} ({f['size'] / 1e6:.1f} MB)", flush=True)
            download(f"https://raw.githubusercontent.com/{REPO}/{REVISION}/{f['path']}", out)
            if git_blob_hash(out) != f["sha"]:
                out.unlink()
                sys.exit(f"{f['path']}: download does not match the pinned Git blob")
    lic = CACHE / "LICENSE.md"
    if not lic.exists():
        download(f"https://raw.githubusercontent.com/{REPO}/{REVISION}/LICENSE.md", lic)


def mpfb():
    zip_path = CACHE / "mpfb" / "mpfb.zip"
    if not zip_path.exists():
        print("MPFB 2.0.17 (45 MB)", flush=True)
        download(MPFB_URL, zip_path)
    if hashlib.sha256(zip_path.read_bytes()).hexdigest() != MPFB_SHA256:
        zip_path.unlink()
        sys.exit("MPFB download does not match the checksum of extensions.blender.org")
    env = dict(os.environ)
    for var, sub_dir in (("BLENDER_USER_RESOURCES", ""), ("BLENDER_USER_CONFIG", "config"),
                         ("BLENDER_USER_SCRIPTS", "scripts"), ("BLENDER_USER_DATAFILES", "datafiles"),
                         ("BLENDER_USER_EXTENSIONS", "extensions")):
        env[var] = str(MPFB_PROFILE / sub_dir)
    # (a cleaned cache leaves the folders behind, empty)
    if not (MPFB_PROFILE / "extensions" / "user_default" / "mpfb" / "__init__.py").exists():
        subprocess.run([find_blender(None), "-b", "--factory-startup", "--command", "extension",
                        "install-file", "-r", "user_default", "-e", str(zip_path)],
                       env=env, check=True, capture_output=True)
    data = MPFB_PROFILE / "extensions" / ".user" / "user_default" / "mpfb" / "data"
    if not any((data / "skins").glob("*/*.mhmat")):
        assets = CACHE / "mpfb" / "makehuman_system_assets_cc0.zip"
        if not assets.exists():
            print("MakeHuman system assets (281 MB)", flush=True)
            download(ASSETS_URL, assets)
        with zipfile.ZipFile(assets) as z:
            z.extractall(data)


def main():
    pax = json.loads((HERE / "pax.json").read_text())
    rocketbox(pax)
    if pax.get("generated"):
        mpfb()


if __name__ == "__main__":
    main()
