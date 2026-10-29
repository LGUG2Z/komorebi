"""W1: can the guest's wallpaper be read and set through IDesktopWallpaper?

usage: python -m platforms.qemu_windows.w1_wallpaper GOLDEN OUT STATE

Puts a PNG in a folder with a space in its name, prints the wallpaper of the
first display, sets it to the PNG, prints it again and, with a second display
plugged in, does the same for that one. A probe: it prints what it saw.
"""

import struct
import sys
import time
import zlib
from pathlib import Path

from .guest import Guest
from .windows_platform import WindowsPlatform


def png(width: int, height: int, rgb: tuple[int, int, int]) -> bytes:
    row = b"\x00" + bytes(rgb) * width

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(
            ">I", zlib.crc32(body))

    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(row * height))
            + chunk(b"IEND", b""))


def main(argv: list[str]) -> int:
    golden, out = Path(argv[0]), Path(argv[1])
    state = Path(argv[2]) if len(argv) > 2 else None
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest", state=state) as guest:
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        path = platform.temp_path("wallpapers\\my pictures\\cool.png")
        platform.put_file(path, png(64, 64, (200, 60, 40)))
        print(f"PASTE put {path}")
        print(f"PASTE display 0 before: {platform.wallpaper(0)!r}")
        platform.set_wallpaper(path, 0)
        print(f"PASTE display 0 after:  {platform.wallpaper(0)!r}")
        platform.add_display()
        time.sleep(5)
        print(f"PASTE display 1 before: {platform.wallpaper(1)!r}")
        platform.set_wallpaper(path, 1)
        print(f"PASTE display 1 after:  {platform.wallpaper(1)!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
