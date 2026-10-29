"""What a scenario needs of a platform: the hands and the eyes, behind methods.

A scenario drives the daemon and looks at the desktop only through a
[`Platform`](komorebi_vm.core.platform.Platform), so that the same scenario runs on KDE and on
Windows. A platform may implement a method on the host (QMP on QEMU) or in the guest (an agent); a
scenario cannot tell.

Only what a scenario uses today is here. A method is added with the first scenario that needs it,
and takes the shape both platforms can give.
"""

from dataclasses import dataclass
from typing import Protocol

from PIL import Image

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.snapshot import Snapshot


@dataclass(frozen=True)
class Client:
    """A window a scenario opened. `title` is its identity: a scenario gives each its own."""

    title: str


@dataclass(frozen=True)
class OsWindow:
    """What the operating system says about a window, which is the oracle of a scenario.

    The daemon's own state is the thing under test and is never its own oracle.
    """

    title: str
    frame: Rect
    """The visible frame: the compositor's on KDE, the DWM's extended frame on Windows."""
    focused: bool


class Platform(Protocol):
    """A machine with a desktop and a daemon to test."""

    @property
    def tolerance(self) -> int:
        """How far a window's frame may be short of the one asked for, in pixels.

        A terminal sizes itself to whole cells; a test window has no granularity.
        """
        ...

    def wait_for(self, predicate: "Predicate", timeout: float = 10) -> bool:
        """Polls `predicate` until it holds or `timeout` seconds, scaled for this host, pass."""
        ...

    def start_komorebi(self) -> None:
        """Starts the daemon from a clean state and waits until it answers."""
        ...

    def komorebic(self, *args: str) -> str:
        """Runs `komorebic` with `args` and returns what it printed. Fails if it does."""
        ...

    def state(self) -> Snapshot:
        """The daemon's state."""
        ...

    def spawn(self, title: str) -> Client:
        """Opens a window with this title and waits until the platform has it."""
        ...

    def os_window(self, client: Client) -> OsWindow | None:
        """What the operating system says about the window, or `None` if it has none."""
        ...

    def hidden(self, client: Client) -> bool:
        """The window exists but is out of sight because the daemon put it away: the other windows
        of a stack, the windows of another workspace, those behind a monocle.

        That is a window parked on a desktop of its own on KDE, and cloaked on Windows. A window
        the user minimized is not hidden in this sense.
        """
        ...

    def work_area(self) -> Rect:
        """The area of the first display that windows may take."""
        ...

    def activate(self, client: Client) -> None:
        """Gives the window the focus, as a click on it would."""
        ...

    def screenshot(self, name: str) -> Image.Image:
        """The display as an image, kept under `name` with the other artifacts of the run."""
        ...


class Predicate(Protocol):
    def __call__(self) -> bool: ...
