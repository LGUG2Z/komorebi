"""W1: where does the QMP tablet put the pointer with two displays?

usage: python -m platforms.qemu_windows.w1_pointer2 GOLDEN OUT

Adds a second display, moves the absolute tablet to a few fractions of its
range and asks Windows where the cursor is. A probe: it prints what it saw.
"""

import sys
import time
from pathlib import Path

from .guest import Guest
from .qmp import POINTER_MAX
from .windows_platform import WindowsPlatform

CURSOR = (
    "Add-Type -AssemblyName System.Windows.Forms; "
    "$p = [System.Windows.Forms.Cursor]::Position; \"$($p.X),$($p.Y)\""
)


def cursor(guest: Guest) -> str:
    return guest.agent.exec(
        "powershell", "-NoProfile", "-Command", CURSOR
    )["stdout"].strip()


def main(argv: list[str]) -> int:
    golden, out = (Path(a) for a in argv[:2])
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest") as guest:
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        platform.add_display()
        time.sleep(3)
        for d in guest.agent.displays():
            print("PASTE display", d["monitor"],
                  "primary" if d["primary"] else "")
        for fraction in (0.0, 0.25, 0.5, 0.75, 1.0):
            value = int(POINTER_MAX * fraction)
            guest.qmp.execute("input-send-event", events=[
                {"type": "abs", "data": {"axis": "x", "value": value}},
                {"type": "abs", "data": {"axis": "y", "value": 8000}},
            ])
            time.sleep(0.5)
            print(f"PASTE tablet x {fraction:.2f} ({value}) -> cursor",
                  cursor(guest))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
