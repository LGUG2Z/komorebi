"""The part of the NixOS test driver's `Machine` that the library uses.

The driver's own class is not importable outside a test run, so the library
depends on this protocol instead, and its unit tests pass a fake. The methods
take only what the library passes; the driver's accept more (a timeout, whether
to check the exit status), which does not stop its class from satisfying the
protocol.
"""

from pathlib import Path
from typing import Protocol


class Machine(Protocol):
    """A virtual machine the test can run commands on, as root."""

    @property
    def out_dir(self) -> Path:
        """The directory that screenshots and other artifacts go to."""
        ...

    def execute(self, command: str, /) -> tuple[int, str]:
        """Runs `command` and returns its exit status and its output."""
        ...

    def succeed(self, *commands: str) -> str:
        """Runs each command and fails the test unless it exits with status 0."""
        ...

    def fail(self, *commands: str) -> str:
        """Runs each command and fails the test if it exits with status 0."""
        ...

    def screenshot(self, filename: str, /) -> None:
        """Saves a PNG of the machine's display as `filename` in `out_dir`."""
        ...

    def send_key(self, key: str, /) -> None:
        """Presses the keys `key` names together on the machine's keyboard (QEMU's `sendkey`)."""
        ...
