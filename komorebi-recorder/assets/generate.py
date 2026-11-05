#!/usr/bin/env python3
"""Renders watermelon.svg into the two files the program uses.

watermelon.png (256 by 256) is the icon every window sets while the program runs, and
watermelon.ico is the icon Windows shows for the executables and for the installer. Needs
`resvg` on PATH, which `nix develop` provides; run it with `just icon`.
"""

import struct
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SIZES = [16, 24, 32, 48, 64, 128, 256]


def render(size, directory):
    out = Path(directory) / f"{size}.png"
    subprocess.run(
        ["resvg", "-w", str(size), "-h", str(size), str(HERE / "watermelon.svg"), str(out)],
        check=True,
    )
    return out.read_bytes()


def ico(images):
    """An ICO file whose entries are PNGs, which Windows has read since Vista. A size of 256 is
    written as 0."""
    offset = 6 + 16 * len(images)
    entries, data = b"", b""
    for size, png in images:
        entries += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset + len(data)
        )
        data += png
    return struct.pack("<HHH", 0, 1, len(images)) + entries + data


with tempfile.TemporaryDirectory() as directory:
    images = [(size, render(size, directory)) for size in SIZES]

(HERE / "watermelon.png").write_bytes(images[-1][1])
(HERE / "watermelon.ico").write_bytes(ico(images))
