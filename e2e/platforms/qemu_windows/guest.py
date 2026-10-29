"""One throwaway Windows guest: an overlay on the golden image, run by QEMU.

The golden image is never written: every run has its own
overlay, its own copy of the firmware variables and of the TPM state, and
all of them are deleted with the run directory.
"""

import os
import shutil
import socket
import subprocess
import time
from pathlib import Path
from types import TracebackType

from .agent import Agent
from .qmp import Qmp

# the guest clock: an evaluation build reads as expired at the host's date
RTC_BASE = "2024-10-01T00:00:00"


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


# a second monitor: a QXL device, which gets the signed qxldod driver from the
# image. Windows' basic display driver ignores a second VGA device.
SECOND_DISPLAY = [
    "-device", "qxl,id=qxl2,ram_size_mb=64,vram_size_mb=64,vgamem_mb=16",
]


# how many displays can be added at runtime
HOTPLUG_PORTS = 6


class Guest:
    """Boots the golden image and gives a `Qmp` and a connected `Agent`.

    Needs `qemu-system-x86_64`, `qemu-img` and `swtpm` on PATH and `OVMF_FD`
    (an OVMF build with TPM support and without SMM) in the environment.
    """

    def __init__(
        self,
        golden: Path,
        run: Path,
        vcpus: int = 4,
        mem: str = "6G",
        extra: list[str] | None = None,
        state: Path | None = None,
        ahci: bool = False,
        rtc_base: str | None = RTC_BASE,
        keep: bool = False,
    ) -> None:
        self._ahci = ahci
        self._rtc_base = rtc_base
        self._keep = keep
        # a saved state is restored instead of booting; it can only be saved
        # from a disk on an AHCI controller (QEMU does not migrate NVMe), so
        # a guest that takes part in saving or restoring uses one
        self._state = state
        self._golden = golden
        self._run = run
        self._vcpus = vcpus
        self._mem = mem
        self._extra = extra or []
        self._procs: list[subprocess.Popen[bytes]] = []
        self.agent_port = _free_port()
        self.qmp: Qmp
        self.agent: Agent

    def __enter__(self) -> "Guest":
        self._run.mkdir(parents=True, exist_ok=True)
        run, golden = self._run, self._golden
        # the sockets of the guest are in `run`, and a unix socket path is
        # limited to 108 bytes: say so, instead of a guest that never answers
        longest = str(run / "tpm.sock")
        if len(longest.encode()) > 100:
            raise RuntimeError(
                f"the run directory path is too long for a socket: {longest}"
            )
        overlay = run / "overlay.qcow2"
        # the base is the golden disk, or the disk as it was when the state
        # was saved: the saved memory belongs to exactly that disk
        base = self._state / "overlay.qcow2" if self._state \
            else golden / "disk.qcow2"
        origin = self._state or golden
        subprocess.run(
            ["qemu-img", "create", "-q", "-f", "qcow2", "-b",
             str(base), "-F", "qcow2", str(overlay)],
            check=True,
        )
        shutil.copy(origin / "vars.fd", run / "vars.fd")
        shutil.copytree(origin / "tpm", run / "tpm", dirs_exist_ok=True)

        self._start(
            ["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={run / 'tpm'}",
             "--ctrl", f"type=unixio,path={run / 'tpm.sock'}"]
        )
        ovmf = Path(os.environ["OVMF_FD"]) / "FV"
        self._start(self.qemu_args(ovmf, run, overlay))
        self.qmp = Qmp(run / "qmp.sock")
        if self._state:
            self._restore()
        self.agent = Agent(port=self.agent_port)
        self.agent.connect(timeout=180)
        return self

    def qemu_args(self, ovmf: Path, run: Path, overlay: Path) -> list[str]:
        """The command line of QEMU. A saved state only fits a guest with the
        same one apart from its paths, so `state.py` hashes it."""
        return ["qemu-system-x86_64", "-name", "e2e-windows", "-accel", "kvm",
                "-machine", "q35,smm=off", "-cpu", "host",
                "-smp", str(self._vcpus), "-m", self._mem,
                *(["-rtc", f"base={self._rtc_base}"] if self._rtc_base else []),
                "-drive", f"if=pflash,format=raw,readonly=on,unit=0,"
                          f"file={ovmf / 'OVMF_CODE.fd'}",
                "-drive", f"if=pflash,format=raw,unit=1,file={run / 'vars.fd'}",
                "-chardev", f"socket,id=chrtpm,path={run / 'tpm.sock'}",
                "-tpmdev", "emulator,id=tpm0,chardev=chrtpm",
                "-device", "tpm-crb,tpmdev=tpm0",
                "-drive", f"if=none,id=disk,file={overlay},format=qcow2",
                *self._disk_device(),
                "-netdev", f"user,id=net0,hostfwd=tcp:127.0.0.1:"
                           f"{self.agent_port}-:7878",
                "-device", "e1000e,netdev=net0",
                "-device", "qemu-xhci", "-device", "usb-tablet",
                "-device", "usb-kbd", "-device", "VGA,vgamem_mb=64",
                # hotplug slots for extra displays (see `WindowsPlatform`)
                *[arg for n in range(1, HOTPLUG_PORTS + 1) for arg in
                  ("-device", f"pcie-root-port,id=rp{n},chassis={n},slot={n}")],
                *self._extra,
                # a QXL device wants a SPICE server behind it; without one QEMU
                # aborts in handle_dev_oom when the device runs short of memory
                "-spice", f"unix=on,addr={run / 'spice.sock'},"
                          "disable-ticketing=on",
                "-display", "none",
                "-qmp", f"unix:{run / 'qmp.sock'},server=on,wait=off",
                *(["-incoming", "defer"] if self._state else [])]

    def _disk_device(self) -> list[str]:
        if self._state is None and not self._ahci:
            return ["-device", "nvme,drive=disk,serial=e2e0,bootindex=1"]
        return ["-device", "ich9-ahci,id=ahci",
                "-device", "ide-hd,drive=disk,bus=ahci.0,bootindex=1"]

    def _restore(self) -> None:
        self.qmp.execute(
            "migrate-incoming", uri=f"exec:cat {self._state / 'state.mig'}"
        )
        deadline = time.monotonic() + 300
        while self.qmp.status() == "inmigrate":
            if time.monotonic() > deadline:
                raise RuntimeError("the saved state did not load")
            time.sleep(0.05)
        self.qmp.execute("cont")

    def save(self, dest: Path) -> None:
        """Saves the running guest to `dest` and stops it: its memory
        (state.mig), the disk as it is now (overlay.qcow2, to be a read-only
        base), the firmware variables and the TPM state."""
        dest.mkdir(parents=True, exist_ok=True)
        self.agent.close()
        self.qmp.execute("stop")
        self.qmp.execute(
            "migrate", uri=f"exec:cat > {dest / 'state.mig'}"
        )
        while True:
            status = self.qmp.execute("query-migrate")["status"]
            if status == "completed":
                break
            if status in ("failed", "cancelled"):
                raise RuntimeError(f"saving the state: migration {status}")
            time.sleep(0.05)
        self.qmp.close()
        self._terminate()
        shutil.move(self._run / "overlay.qcow2", dest / "overlay.qcow2")
        shutil.copy(self._run / "vars.fd", dest / "vars.fd")
        shutil.copytree(self._run / "tpm", dest / "tpm", dirs_exist_ok=True)

    def _start(self, argv: list[str]) -> None:
        # what a program writes to stderr is kept: it says why QEMU aborted
        log = self._run / f"{Path(argv[0]).name}.err"
        with open(log, "wb") as err:
            self._procs.append(subprocess.Popen(argv, stderr=err))

    def died(self) -> str | None:
        """Why QEMU has exited, if it has while the guest was in use (a signal
        that killed it, and the last of what it wrote), else `None`."""
        for proc in self._procs:
            code = proc.poll()
            if code is None or proc.args[0] != "qemu-system-x86_64":
                continue
            log = self._run / "qemu-system-x86_64.err"
            tail = log.read_text(errors="replace").strip().splitlines()[-5:] \
                if log.exists() else []
            how = f"signal {-code}" if code < 0 else f"status {code}"
            return f"QEMU exited with {how}: " + " | ".join(tail)
        return None

    def upload(self, local_dir: Path, names: list[str]) -> None:
        """Puts binaries under test in the guest's `C:\\e2e\\bin`."""
        self.agent.exec("cmd", "/c", "mkdir C:\\e2e\\bin 2>nul")
        for name in names:
            self.agent.put_file(local_dir / name, f"C:\\e2e\\bin\\{name}")

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        try:
            self.agent.close()
            self.qmp.close()
        except Exception:
            pass
        self._terminate()
        # the overlay, the firmware variables and the TPM state of a run are
        # several hundred megabytes each; the artifacts that matter are in
        # the output directory, outside this one
        if not self._keep and not os.environ.get("KOMOREBI_E2E_KEEP_GUEST"):
            shutil.rmtree(self._run, ignore_errors=True)

    def wait_exit(self, timeout: float) -> bool:
        """Waits for QEMU to exit, as it does when the guest shuts itself
        down; True if it did."""
        try:
            self._procs[-1].wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return False
        return True

    def _terminate(self) -> None:
        # our own children only, by handle: never by name
        for proc in reversed(self._procs):
            proc.terminate()
        for proc in self._procs:
            try:
                proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
