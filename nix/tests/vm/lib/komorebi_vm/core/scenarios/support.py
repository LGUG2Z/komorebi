"""What more than one scenario needs of a platform."""

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform

# how long something that must not happen is waited for, in seconds (scaled for the host): a window
# a rule leaves alone, a window that stays where it is. It is more than the daemon takes to act on
# an event on either port, and the price of every such check
QUIET = 3


def os_focused(platform: Platform, clients: list[Client]) -> str:
    """The title of the window the operating system says has the focus, among `clients`.

    The empty string when none of them does.
    """
    for client in clients:
        window = platform.os_window(client)
        if window is not None and window.focused:
            return window.title
    return ""


def in_view(platform: Platform, clients: list[Client]) -> list[str]:
    """The titles of the windows of `clients` that are on the screen: the operating system has
    them, and the daemon has not put them away. Sorted.

    A window the system does not have is not in view, though `Platform.hidden` says no for it
    too: a window that is gone must not count as one that is shown.
    """
    return sorted(
        c.title for c in clients if platform.os_window(c) is not None and not platform.hidden(c)
    )


def positions(platform: Platform, clients: list[Client]) -> dict[str, tuple[int, int]]:
    """Where the operating system has each window, by title.

    Only the position: a client may round its size to what suits it (a terminal's cells), and
    a window that moved is told by where it is.
    """
    found = {}
    for client in clients:
        window = platform.os_window(client)
        if window is not None:
            found[client.title] = (window.frame.x, window.frame.y)
    return found


def focus_agrees(platform: Platform, clients: list[Client], title: str) -> bool:
    """The operating system and the daemon's state both have `title` focused."""
    return os_focused(platform, clients) == title and platform.state().focused_title() == title


def layout_rect(platform: Platform, client: Client) -> Rect | None:
    """The rectangle the daemon's layout gives the container of `client`, or `None` if the
    first workspace has no such container.

    It is what the daemon asked for: the frame the operating system gives the window may be short
    of it by `platform.tolerance`, and is what the scenario then compares it with.
    """
    workspace = platform.state().workspace()
    for i, stack in enumerate(workspace.containers):
        if any(w.title == client.title for w in stack) and i < len(workspace.latest_layout):
            return workspace.latest_layout[i]
    return None
