"""W1: what is the ring of a window when the wallpaper scenario says it is not warm?

usage: python -m platforms.qemu_windows.w1_wallpaper_ring GOLDEN BIN_DIR OUT STATE

Runs `wallpaper.run` with `ring_hue` wrapped to print, for every call, the
frame, the hue and the colors of the pixels it looked at, and keeps the screen
of each call. A probe: it prints what it saw.
"""

import importlib
import sys
from pathlib import Path

from komorebi_vm.core import pixels

from .guest import Guest
from .run import BINARIES, RecordedChecks
from .windows_platform import WindowsPlatform


def main(argv: list[str]) -> int:
    golden, bin_dir, out, state = (Path(a) for a in argv[:4])
    out.mkdir(parents=True, exist_ok=True)
    wallpaper = importlib.import_module("komorebi_vm.core.scenarios.wallpaper")
    original = wallpaper.ring_hue
    calls = []

    def ring_hue(frame, image, reach=12):
        hue = original(frame, image, reach)
        y = frame.y + frame.h // 2
        row = [pixels.pixel(image, x, y) for x in range(
            max(0, frame.x - reach), min(image.size[0], frame.x + reach), 2)]
        calls.append((frame, hue))
        image.save(out / f"ring-{len(calls)}.png")
        print(f"PASTE ring call {len(calls)}: frame {frame} hue {hue}")
        print(f"PASTE   pixels at y={y} from x={frame.x - reach}: {row}")
        return hue

    wallpaper.ring_hue = ring_hue
    with Guest(golden, out / "guest", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        checks = RecordedChecks()
        wallpaper.run(platform, checks)
        print(f"PASTE passed {checks.passed} failed {checks.failed}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
