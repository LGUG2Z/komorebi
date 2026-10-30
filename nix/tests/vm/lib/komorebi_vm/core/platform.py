"""What a scenario needs of a platform: the hands and the eyes, behind methods.

A scenario drives the daemon and looks at the desktop only through a
[`Platform`](komorebi_vm.core.platform.Platform), so that the same scenario runs on KDE and on
Windows. A platform may implement a method on the host (QMP on QEMU) or in the guest (an agent); a
scenario cannot tell.

Only what a scenario uses today is here. A method is added with the first scenario that needs it,
and takes the shape both platforms can give.
"""

from dataclasses import dataclass
from typing import Any, Protocol

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


class Pointer(Protocol):
    """A pointer a scenario moves and presses, in the pixels of the first display.

    A drag is one press, a glide to the end with the button held, and one release, because a
    window manager that is asked to follow the pointer drops moves that come a round trip apart.
    """

    def move(self, x: int, y: int) -> None:
        """Puts the pointer on the pixel (x, y)."""
        ...

    def down(self) -> None:
        """Presses the left button."""
        ...

    def up(self) -> None:
        """Lets go of the left button."""
        ...

    def drag(
        self, start: tuple[int, int], end: tuple[int, int], steps: int = 25, ms: int = 30
    ) -> None:
        """Presses the left button at `start`, glides to `end` with it held, and lets go there."""
        ...


class Platform(Protocol):
    """A machine with a desktop and a daemon to test."""

    def has(self, feature: str) -> bool:
        """Whether the platform can do what a scenario needs of it (`"pointer"`, `"lock"`), so
        that a scenario that needs it is skipped, with the reason, where it cannot."""
        ...

    @property
    def pointer(self) -> Pointer:
        """The pointer. Only where `has("pointer")`."""
        ...

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

    def restart_with(self, config: dict[str, Any]) -> None:
        """Stops the daemon if it runs, and starts it from a clean state with `config` as its
        `komorebi.json`, and waits until it answers.

        Windows that are open stay open, and the new daemon takes up the ones that are in view.
        """
        ...

    def rewrite_config(self, config: dict[str, Any]) -> None:
        """Writes `config` over the `komorebi.json` that `restart_with` gave the daemon, which
        the daemon watches and reloads when it changes. It returns once the file is written; a
        scenario waits for the state to follow."""
        ...

    def komorebic(self, *args: str) -> str:
        """Runs `komorebic` with `args` and returns what it printed. Fails if it does."""
        ...

    def komorebic_status(self, *args: str) -> tuple[int, str]:
        """Runs `komorebic` with `args` and returns its exit status and what it printed, whether
        it succeeded or not."""
        ...

    def state(self) -> Snapshot:
        """The daemon's state."""
        ...

    def spawn(self, title: str, app_id: str | None = None, exe: str | None = None) -> Client:
        """Opens a window with this title and waits until the platform has it.

        `app_id` is the class of the window (its app id on Wayland, its window class on Windows)
        for a rule that matches by it; the platform's own when there is none.

        `exe` is the name of the program that opens it, without an extension (`e2e-exe` is
        `e2e-exe.exe` on Windows and `e2e-exe` on Linux), for a rule that matches by it. The
        platform makes a program of that name, and `exe_id` spells the id of a rule for it.
        """
        ...

    def exe_id(self, program: str) -> str:
        """The `id` of a rule that matches the windows of the program `spawn(exe=program)` opens:
        the program's file name, which is `program` on Linux and `program.exe` on Windows."""
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

    def displays(self) -> list[Rect]:
        """The displays the operating system has, left to right: each one's rectangle in the
        coordinates of the desktop. The daemon is not asked."""
        ...

    def add_display(self) -> None:
        """Plugs in another display, to the right of the others, and returns when the operating
        system has it. The daemon learns of it later, so a scenario waits for its state to follow.
        A platform may give it another size than the others; `displays()` says which."""
        ...

    def remove_display(self) -> None:
        """Unplugs the display that was plugged in last, and returns when the operating system no
        longer has it."""
        ...

    def close(self, client: Client) -> None:
        """Closes the window, as its close button would."""
        ...

    def minimize(self, client: Client) -> None:
        """Minimizes the window, as its title bar button would. `activate` restores it."""
        ...

    def lock(self) -> None:
        """Locks the screen, as the user's lock shortcut would, and returns when it is locked.
        Only where `has("lock")`."""
        ...

    def unlock(self) -> None:
        """Unlocks the screen, as the user's password would, and returns when it is unlocked.
        Only where `has("lock")`."""
        ...

    def move_window(self, client: Client, x: int, y: int) -> None:
        """Moves the window so that its top left corner is at (x, y) of the desktop, as the
        compositor or the shell would, not as the user's pointer would. The window keeps its
        size."""
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
