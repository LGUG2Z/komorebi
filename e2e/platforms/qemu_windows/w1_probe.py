"""W1: does QMP pointer input drive a Windows guest, and is there a second
display?

usage: python -m platforms.qemu_windows.w1_probe GOLDEN BIN_DIR OUT

Prints a PASTE block. Draft of a probe, not a test: it records what it saw.
"""

import sys
import time
from pathlib import Path

from .guest import Guest


def _frame(agent, title):
    for w in agent.os_windows():
        if w["title"] == title:
            return w["frame"]
    return None


def pointer(golden: Path, bin_dir: Path, out: Path) -> None:
    with Guest(golden, out / "pointer") as guest:
        guest.upload(bin_dir, ["komorebi-testwin.exe"])
        agent, qmp = guest.agent, guest.qmp
        agent.spawn("C:\\e2e\\bin\\komorebi-testwin.exe", "--title", "w1")
        time.sleep(3)
        before = _frame(agent, "w1")
        print("PASTE pointer before", before)
        display = agent.displays()[0]["monitor"]
        w, h = display["right"], display["bottom"]
        # the middle of the title bar, 15 px below the frame's top edge
        x = before["left"] + before["right"] // 2
        y = before["top"] + 15
        qmp.pointer_move(x, y, w, h)
        time.sleep(0.3)
        qmp.pointer_button(True)
        for step in range(1, 11):
            qmp.pointer_move(x + 20 * step, y + 10 * step, w, h)
            time.sleep(0.05)
        qmp.pointer_button(False)
        time.sleep(1)
        after = _frame(agent, "w1")
        print("PASTE pointer after", after)
        moved = (after["left"] - before["left"], after["top"] - before["top"])
        print("PASTE pointer drag moved the window by", moved,
              "(asked for (200, 100))")
        qmp.screendump(out / "pointer-after.png")


def second_display(golden: Path, out: Path) -> None:
    extra = ["-device", "secondary-vga,id=vga2"]
    with Guest(golden, out / "display", extra=extra) as guest:
        time.sleep(5)
        shown = guest.agent.displays()
        print("PASTE displays seen by Windows with a second VGA:", len(shown))
        for d in shown:
            print("PASTE   ", d)


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv)
    out.mkdir(parents=True, exist_ok=True)
    pointer(golden, bin_dir, out)
    second_display(golden, out)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
