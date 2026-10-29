"""W1: a second monitor from a second QXL device, with Red Hat's signed
`qxldod` display driver from virtio-win.

usage: python -m platforms.qemu_windows.w1_qxl GOLDEN QXL_ISO OUT [BIN_DIR]

QXL_ISO holds the files of virtio-win's qxldod/w10/amd64. Prints what it
saw; a probe, not a test.
"""

import sys
import time
from pathlib import Path

from .guest import Guest
from .windows_platform import WindowsPlatform

QXL = "qxl,id=qxl2,ram_size_mb=64,vram_size_mb=64,vgamem_mb=16"
POWERSHELL = ["powershell", "-NoProfile", "-Command"]


def show(guest: Guest, label: str) -> None:
    print(f"PASTE {label}: displays", len(guest.agent.displays()))
    for d in guest.agent.displays():
        print("PASTE    ", d)
    video = guest.agent.exec(
        *POWERSHELL,
        "Get-CimInstance Win32_VideoController | "
        "ForEach-Object { '{0} | {1} | code {2}' -f "
        "$_.Name, $_.Status, $_.ConfigManagerErrorCode }",
    )
    print(f"PASTE {label}: video controllers\n{video['stdout']}")


def main(argv: list[str]) -> int:
    golden, qxl_iso, out = (Path(a) for a in argv[:3])
    bin_dir = Path(argv[3]) if len(argv) > 3 else None
    out.mkdir(parents=True, exist_ok=True)
    extra = [
        "-device", QXL,
        "-drive", f"if=none,id=qd,file={qxl_iso},media=cdrom,readonly=on",
        "-device", "ide-cd,drive=qd,bus=ide.0",
    ]
    with Guest(golden, out / "guest", extra=extra) as guest:
        time.sleep(5)
        show(guest, "before the driver")
        found = guest.agent.exec(
            *POWERSHELL,
            "(Get-PSDrive -PSProvider FileSystem | Where-Object "
            "{ Test-Path (Join-Path $_.Root 'qxldod.inf') }).Root",
        )["stdout"].strip()
        print("PASTE driver CD at", repr(found))
        added = guest.agent.exec("pnputil", "/add-driver", f"{found}qxldod.inf",
                                 "/install")
        print("PASTE pnputil /add-driver:", added["status"],
              added["stdout"][-600:], added["stderr"][-300:])
        guest.agent.exec("pnputil", "/scan-devices")
        time.sleep(15)
        show(guest, "after the driver")
        guest.qmp.screendump(out / "after.png")
        if bin_dir:
            guest.upload(bin_dir, ["komorebi.exe", "komorebic.exe"])
            platform = WindowsPlatform(guest.qmp, guest.agent, out)
            platform.start_komorebi()
            state = platform.state()
            print("PASTE komorebi monitors:", len(state.monitors))
            for m in state.monitors:
                print("PASTE    ", m.id, m.size, m.work_area)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
