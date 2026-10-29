"""Builds the golden layer on top of a base image.

usage: python -m platforms.qemu_windows.build_golden BASE OUT QXLDOD_DIR

Boots an overlay of BASE with the host's clock (the Visual C++ download needs
TLS), provisions it through the agent, has the guest shut itself down, and
leaves OUT/disk.qcow2 backed by BASE through a relative path, with the
firmware variables and the TPM state as they are after that shutdown.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

from .guest import Guest

HERE = Path(__file__).parent
DRIVER_FILES = ("qxldod.inf", "qxldod.cat", "qxldod.sys")


def main(argv: list[str]) -> int:
    base, out, driver = (Path(a).resolve() for a in argv[:3])
    shutil.rmtree(out, ignore_errors=True)
    # the overlay, firmware variables and TPM state of the run are the
    # artifacts, so the run directory stays
    with Guest(base, out, rtc_base=None, keep=True) as guest:
        agent = guest.agent
        agent.exec("cmd", "/c", "mkdir C:\\e2e\\drv 2>nul")
        for name in DRIVER_FILES:
            agent.put_file(driver / name, f"C:\\e2e\\drv\\{name}")
        agent.put_file(HERE / "provision.ps1", "C:\\e2e\\provision.ps1")
        result = agent.exec(
            "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
            "-File", "C:\\e2e\\provision.ps1",
        )
        print(agent.get_file("C:\\e2e-provision.log").decode())
        if result["status"] != 0:
            raise RuntimeError(f"provisioning failed: {result}")
        agent.exec("cmd", "/c", "shutdown /s /t 0")
        agent.close()
        if not guest.wait_exit(300):
            raise RuntimeError("the guest did not shut down")
    # the swtpm of the run has been stopped, so its state is flushed
    for sock in out.glob("*.sock"):
        sock.unlink()
    disk = out / "disk.qcow2"
    (out / "overlay.qcow2").rename(disk)
    relative = os.path.relpath(base / "disk.qcow2", out)
    subprocess.run(
        ["qemu-img", "rebase", "-u", "-b", relative, "-F", "qcow2", str(disk)],
        check=True,
    )
    disk.chmod(0o444)
    print(f"golden image: {disk} on {relative}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
