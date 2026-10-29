"""Where does a drag fall short: the input path, or the daemon?

usage: python -m platforms.qemu_windows.w1_drag GOLDEN BIN_DIR OUT

Drags the right border of a plain `komorebi-testwin` that no daemon manages,
by 30, 60 and 120 px, and prints the cursor Windows reports after each drag,
the frame during the glide and the frame at the end. A probe: it records what
it saw.
"""

import sys
import time
from pathlib import Path

from .guest import Guest
from .qmp import Qmp

CURSOR = (
    "Add-Type -AssemblyName System.Windows.Forms; "
    "$p = [System.Windows.Forms.Cursor]::Position; '{0},{1}' -f $p.X, $p.Y"
)


def frame(guest: Guest):
    for w in guest.agent.os_windows():
        if w["title"] == "w1":
            return w["frame"]
    return None


def cursor(guest: Guest) -> str:
    out = guest.agent.exec("powershell", "-NoProfile", "-Command", CURSOR)
    return out["stdout"].strip()


def drag(guest: Guest, qmp: Qmp, start, end, size, steps=25, ms=30):
    width, height = size
    qmp.pointer_move(*start, width, height)
    time.sleep(0.3)
    qmp.pointer_button(True)
    samples = []
    for i in range(1, steps + 1):
        x = start[0] + (end[0] - start[0]) * i // steps
        y = start[1] + (end[1] - start[1]) * i // steps
        qmp.pointer_move(x, y, width, height)
        time.sleep(ms / 1000)
        if i % 5 == 0:
            f = frame(guest)
            samples.append((x, f["left"] + f["right"] if f else None))
    after_glide = frame(guest)
    qmp.pointer_button(False)
    time.sleep(0.5)
    return samples, after_glide


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv)
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest") as guest:
        guest.upload(bin_dir, ["komorebi-testwin.exe"])
        guest.agent.spawn(
            "C:\\e2e\\bin\\komorebi-testwin.exe", "--title", "w1",
            "--size", "400x300",
        )
        time.sleep(3)
        monitor = guest.agent.displays()[0]["monitor"]
        size = (monitor["right"], monitor["bottom"])
        for dx in (30, 60, 120):
            f = frame(guest)
            right = f["left"] + f["right"]
            start = (right + 4, f["top"] + f["bottom"] // 2)
            end = (start[0] + dx, start[1])
            samples, glide = drag(guest, guest.qmp, start, end, size)
            after = frame(guest)
            print(f"PASTE dx={dx} asked end x={end[0]}")
            print(f"PASTE   cursor after release: {cursor(guest)}")
            print(f"PASTE   right edge before {right}, "
                  f"at end of glide {glide['left'] + glide['right']}, "
                  f"after release {after['left'] + after['right']}, "
                  f"gain {after['left'] + after['right'] - right}")
            print(f"PASTE   samples (pointer x, frame right edge): {samples}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
