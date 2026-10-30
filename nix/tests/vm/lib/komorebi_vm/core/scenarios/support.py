"""What more than one scenario needs of a platform."""

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform


def os_focused(platform: Platform, clients: list[Client]) -> str:
    """The title of the window the operating system says has the focus, among `clients`.

    The empty string when none of them does.
    """
    for client in clients:
        window = platform.os_window(client)
        if window is not None and window.focused:
            return window.title
    return ""


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
