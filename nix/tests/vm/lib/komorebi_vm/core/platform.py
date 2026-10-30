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
    """The visible frame: the compositor's on KDE, the DWM's extended frame on Windows, without
    the invisible borders and the shadow. A scenario does not read the frame of a window that is
    minimized: it is where the system keeps it, which is not the same on every port."""
    focused: bool
    """The window has the keyboard focus, as the operating system says. At most one window of a
    desktop does; none does while another program's window has it."""


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

        A client that sizes itself to a grid (a terminal's cells) ends up short of the tile; one
        that does not (a test window) has only the rounding of the system's own frame. The frame
        is never past what was asked, so a check that something is inside an area allows no slack
        for it, and a check that two sizes are the same allows this much.

        It is a bound on what a client rounds to, not a measure of a change: a scenario that
        asks whether the daemon changed a size compares the daemon's layout (`latest_layout`,
        which no client rounds) and then that the frame follows it within this.
        """
        ...

    def wait_for(self, predicate: "Predicate", timeout: float = 10) -> bool:
        """Polls `predicate` until it holds or `timeout` seconds, scaled for this host, pass.

        A predicate answers false for what it cannot tell yet. A port may treat a `komorebic`
        that fails, in a predicate, as a false (Windows does, while the host is busy), and one may
        not (KDE raises): a predicate does not rely on either.
        """
        ...

    def start_komorebi(self) -> None:
        """Starts the daemon from a clean state and waits until it answers."""
        ...

    def crash(self) -> None:
        """Kills the daemon abruptly, as a crash does, and returns when it is gone. Nothing of
        what it had set up is put back: a window it hid is as it left it, until a daemon starts
        again and finds it."""
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

    def temp_path(self, name: str) -> str:
        """A path, on the machine the daemon runs on, of a file `komorebic` and the daemon may
        write and read (`save-resize` takes one), in the form `komorebic` is given it."""
        ...

    def state(self) -> Snapshot:
        """The daemon's state."""
        ...

    def spawn(
        self,
        title: str,
        app_id: str | None = None,
        exe: str | None = None,
        rename_to: str | None = None,
        after: float = 0,
    ) -> Client:
        """Opens a window with this title and waits until the platform has it, and raises if it
        never does: a scenario that waits for a window to stay unmanaged must be sure it is there.

        A title is a name that no other window has, taken as it is: it is never a pattern.

        `app_id` is the class of the window (its app id on Wayland, its window class on Windows)
        for a rule that matches by it; the platform's own when there is none.

        `exe` is the name of the program that opens it, without an extension (`e2e-exe` is
        `e2e-exe.exe` on Windows and `e2e-exe` on Linux), for a rule that matches by it. The
        platform makes a program of that name, and `exe_id` spells the id of a rule for it.

        `rename_to` is a title the window gives itself `after` seconds after it opens (a terminal
        escape on KDE, a title update on Windows), as a program that sets its title late does. The
        window is then known to the platform by the new title: `Client(rename_to)`.
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
        of a stack, the windows of a workspace that is not the one its display shows, those
        behind a monocle.

        It is false for a window the system does not have, for one the user minimized, and for one
        that floats. A scenario that asks for a window to be put away also asks the daemon where
        it is (`state()`), because a port may answer true for any window that is not on the
        screen, whoever took it off: on KDE that is one on a virtual desktop that is not shown,
        on Windows a cloaked one, which includes the windows of another virtual desktop. A scenario
        that asks for windows to be in view counts those the system has (`support.in_view`).
        """
        ...

    def displays(self) -> list[Rect]:
        """The displays the operating system has, left to right: each one's rectangle in the
        coordinates of the desktop. The daemon is not asked.

        Displays are not the same size (the one `add_display` plugs in may be smaller than the
        first) and need not touch: a scenario uses each one's own rectangle and does not compute
        where the next begins."""
        ...

    def add_display(self) -> None:
        """Plugs in another display, to the right of the others, and returns when the operating
        system has it. The daemon learns of it later, so a scenario waits for its state to follow.
        A platform may give it another size than the others; `displays()` says which."""
        ...

    def remove_display(self) -> None:
        """Unplugs the last display (the rightmost, which is the one `add_display` plugged in last),
        and returns when the operating system no longer has it.

        Only a display `add_display` plugged in is certain to be removable: Windows refuses
        any other, KDE unplugs the last of the outputs it has. A scenario unplugs a display only
        after it plugged one in, unless its suite started the platform with more than one."""
        ...

    def resizable_display(self) -> int | None:
        """The place in `displays()` of a display whose mode (its resolution) can be changed, or
        `None` when the platform has none that can, and a scenario that needs one is skipped with
        the reason. A scenario plugs in a display with `add_display` before it asks."""
        ...

    def resize_display(self, index: int, width: int, height: int) -> None:
        """Gives the display `resizable_display` named another mode, `width` by `height` pixels,
        and returns when the operating system has it: `displays()` says so. The daemon learns of
        it later, so a scenario waits for its state to follow."""
        ...

    def close(self, client: Client) -> None:
        """Closes the window, as its close button would, and returns without waiting for it to be
        gone. Raises if there is no such window."""
        ...

    def minimize(self, client: Client) -> None:
        """Minimizes the window, as its title bar button would. `activate` restores it. Raises if
        there is no such window."""
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

    def fullscreen(self, client: Client, on: bool = True) -> None:
        """Takes the window fullscreen, as the program's own key does it (`on`), or brings it back
        from fullscreen (`on` false). The daemon is not asked: it learns of it from the system, and
        has to leave the window alone. Returns when the system has done it."""
        ...

    def user_maximize(self, client: Client, on: bool = True) -> None:
        """Maximizes the window, as its title bar button does (`on`), or restores it from being
        maximized (`on` false). The daemon is not asked: it learns of it from the system. Returns
        when the system has done it."""
        ...

    def work_area(self) -> Rect:
        """The area of the first display that windows may take, in the coordinates of the
        desktop: the display less what the operating system keeps for itself (a panel, a dock, the
        taskbar), as the operating system says and not the daemon.

        It is the whole display only where there is nothing of the kind. A scenario that asks
        whether a window is "inside the work area" asks this, so a platform that answers with the
        display where the system has a bar makes that check weaker than it says."""
        ...

    def activate(self, client: Client) -> None:
        """Gives the window the focus, as a click on it would, and restores it if it was minimized.
        Raises if there is no such window."""
        ...

    def screenshot(self, name: str) -> Image.Image:
        """The display as an image, kept under `name` with the other artifacts of the run."""
        ...


class Predicate(Protocol):
    def __call__(self) -> bool: ...
