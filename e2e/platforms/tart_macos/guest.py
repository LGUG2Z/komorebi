"""One macOS guest: a clone of the golden Tart image, booted without a window, and the agent in it.

The image logs in on its own and starts the agent from a Terminal window at login (a launch agent
of the user, `build_golden.py`), so that the agent, and the daemon it starts, are attributed to the
program that holds the Accessibility and Screen Recording grants. The host needs nothing but the
address of the guest: no SSH, no password.

Every scenario gets its own guest, cloned from the golden image (copy on write, so a clone costs
nothing), and deleted when the scenario ends. A host may run two guests at once, at most (Apple's
licence and Virtualization.framework both stop a third).
"""

import itertools
import os
import shutil
import subprocess
from pathlib import Path
from typing import Self

from komorebi_vm.agent import Agent, AgentError

# where the host driver puts what it uploads, and where the guest keeps what it writes
GUEST_ROOT = "/Users/admin/e2e"
GUEST_BIN = f"{GUEST_ROOT}/bin"
AGENT_PORT = 7878
# the agent answers about 20 s after the clone starts; the rest is margin for a busy host
BOOT_TIMEOUT = 180

_names = itertools.count(1)


def tart() -> str:
    """The `tart` program: `KOMOREBI_E2E_TART`, or the one on the path."""
    found = os.environ.get("KOMOREBI_E2E_TART") or shutil.which("tart")
    if not found:
        raise RuntimeError(
            "no tart: put it on the path or set KOMOREBI_E2E_TART "
            "(nixpkgs has it, as an unfree package)"
        )
    return found


def _run(*args: str, timeout: float = 120) -> str:
    done = subprocess.run(
        [tart(), *args], capture_output=True, text=True, timeout=timeout, check=False
    )
    if done.returncode != 0:
        raise RuntimeError(f"tart {' '.join(args)}: {done.stderr.strip() or done.stdout.strip()}")
    return done.stdout.strip()


class Guest:
    """A clone of `golden`, running, with its agent connected."""

    def __init__(self, golden: str, out: Path) -> None:
        self._golden = golden
        self._out = out
        # short and unique: it is the name of a directory of the host
        self.name = f"e2e-{os.getpid()}-{next(_names)}"
        self._process: subprocess.Popen | None = None
        self._log = None
        self.ip = ""
        self.agent: Agent | None = None

    def __enter__(self) -> Self:
        try:
            self.start()
        except BaseException:
            self.stop()
            raise
        return self

    def __exit__(self, *_exc: object) -> None:
        self.stop()

    def start(self) -> None:
        self._out.mkdir(parents=True, exist_ok=True)
        _run("clone", self._golden, self.name)
        self._log = (self._out / "tart-run.log").open("wb")
        self._process = subprocess.Popen(
            [tart(), "run", self.name, "--no-graphics"],
            stdout=self._log, stderr=subprocess.STDOUT,
        )
        self.ip = _run("ip", self.name, "--wait", str(BOOT_TIMEOUT), timeout=BOOT_TIMEOUT + 10)
        self.agent = Agent(self.ip, AGENT_PORT)
        self.agent.connect(timeout=BOOT_TIMEOUT)

    def died(self) -> str | None:
        """Why the guest is gone, or `None` while it runs: not what the daemon did."""
        if self._process is not None and self._process.poll() is not None:
            return f"tart run exited with {self._process.returncode}"
        return None

    def upload(self, bin_dir: Path, names: list[str]) -> None:
        """Puts programs of the host's `bin_dir` in the guest, where they can run."""
        assert self.agent is not None
        self.agent.exec("mkdir", "-p", GUEST_BIN)
        for name in names:
            remote = f"{GUEST_BIN}/{name}"
            self.agent.put_file(bin_dir / name, remote)
            self.agent.exec("chmod", "+x", remote)

    def stop(self) -> None:
        """Stops and deletes the clone. Never raises: it runs where something else already did."""
        if self.agent is not None:
            self.agent.close()
        try:
            _run("stop", self.name, timeout=60)
        except (RuntimeError, subprocess.TimeoutExpired):
            pass
        if self._process is not None:
            try:
                self._process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self._process.kill()
        if self._log is not None:
            self._log.close()
        try:
            _run("delete", self.name)
        except (RuntimeError, subprocess.TimeoutExpired) as e:
            print(f"     could not delete the clone {self.name}: {e}")


__all__ = ["AGENT_PORT", "GUEST_BIN", "GUEST_ROOT", "AgentError", "Guest", "tart"]
