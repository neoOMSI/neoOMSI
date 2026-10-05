#!/usr/bin/env python3
"""Render the standalone brand symbol into the platform icon formats."""

from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "assets/logos/icon-gradient.svg"
OUTPUT = ROOT / "assets/icons/app"
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)


def png(directory: Path, size: int) -> bytes:
    return (directory / f"icon-{size}.png").read_bytes()


def write_ico(directory: Path) -> None:
    sizes = (256, 128, 64, 48, 32, 24, 16)
    offset = 6 + 16 * len(sizes)
    entries = []
    images = []
    for size in sizes:
        image = png(directory, size)
        entries.append(
            struct.pack(
                "<BBBBHHII",
                0 if size == 256 else size,
                0 if size == 256 else size,
                0,
                0,
                1,
                32,
                len(image),
                offset,
            )
        )
        images.append(image)
        offset += len(image)
    (OUTPUT / "neoomsi.ico").write_bytes(
        struct.pack("<HHH", 0, 1, len(sizes)) + b"".join(entries + images)
    )


def write_icns(directory: Path) -> None:
    types = {
        16: b"icp4",
        32: b"icp5",
        64: b"icp6",
        128: b"ic07",
        256: b"ic08",
        512: b"ic09",
        1024: b"ic10",
    }
    chunks = []
    for size, kind in types.items():
        image = png(directory, size)
        chunks.append(kind + struct.pack(">I", len(image) + 8) + image)
    payload = b"".join(chunks)
    (OUTPUT / "neoomsi.icns").write_bytes(
        struct.pack(">4sI", b"icns", len(payload) + 8) + payload
    )


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="neoomsi-icons-") as temporary:
        directory = Path(temporary)
        for size in SIZES:
            subprocess.run(
                ["resvg", "-w", str(size), str(SOURCE), str(directory / f"icon-{size}.png")],
                check=True,
            )
        shutil.copyfile(directory / "icon-256.png", OUTPUT / "neoomsi-256.png")
        shutil.copyfile(directory / "icon-512.png", OUTPUT / "neoomsi-512.png")
        write_ico(directory)
        write_icns(directory)
    print(f"icons written to {OUTPUT.relative_to(ROOT)} from {SOURCE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
