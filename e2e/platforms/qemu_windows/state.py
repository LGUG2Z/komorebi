"""A saved running guest, to restore instead of booting for every scenario.

Booting the golden image takes 50 to 90 seconds; restoring a state saved
after the boot takes about 3 (see W1.md). The state is the guest's memory,
the disk as it was then, the firmware variables and the TPM state, and it
only fits the golden image, the QEMU and the command line it was saved with,
so `ensure` makes a new one when any of them changed.
"""

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from .guest import Guest

# the guest sits idle this long after the agent answers, before it is saved
SETTLE_SECONDS = 20


def _chain(disk: Path) -> list[str]:
    """The size and modification time of a disk and of every image it is
    backed by: a golden layer on a base changes when either does."""
    parts = []
    while True:
        stat = disk.stat()
        parts.append(f"{disk.name} {stat.st_size} {stat.st_mtime_ns}")
        info = json.loads(subprocess.run(
            ["qemu-img", "info", "--output=json", str(disk)],
            capture_output=True, text=True, check=True,
        ).stdout)
        backing = info.get("backing-filename")
        if not backing:
            return parts
        disk = (disk.parent / backing).resolve()


def _machine() -> list[str]:
    """The command line of the guest that is saved, with the paths and the
    port, which differ from run to run, as placeholders: a change to what the
    guest is (its devices, memory, clock) makes a new state, and a change to
    the rest of guest.py does not."""
    guest = Guest(Path("golden"), Path("run"), ahci=True)
    guest.agent_port = 0
    return guest.qemu_args(Path("ovmf"), Path("run"), Path("run/overlay.qcow2"))


def _key(golden: Path) -> str:
    qemu = subprocess.run(
        ["qemu-system-x86_64", "--version"], capture_output=True, text=True,
        check=True,
    ).stdout
    parts = [
        *_chain(golden / "disk.qcow2"), qemu,
        os.environ.get("OVMF_FD", ""), json.dumps(_machine()),
    ]
    return hashlib.sha256("\n".join(parts).encode()).hexdigest()


def ensure(golden: Path, state: Path) -> Path:
    """Returns `state`, saved from `golden` if it is not there or is stale.

    One process at a time: nothing guards against two making it together.
    """
    key = _key(golden)
    marker = state / "key"
    if marker.exists() and marker.read_text() == key:
        return state
    shutil.rmtree(state, ignore_errors=True)
    print(f"== saving a state of the guest to {state}", flush=True)
    started = time.monotonic()
    # next to the state, not in $TMPDIR: the sockets of the guest are in it,
    # and a socket path is limited to about 100 characters
    with tempfile.TemporaryDirectory(dir=state.parent, prefix=".build-") as run:
        with Guest(golden, Path(run), ahci=True) as guest:
            time.sleep(SETTLE_SECONDS)
            guest.save(state)
    marker.write_text(key)
    print(f"   saved after {time.monotonic() - started:.0f} s", flush=True)
    return state
