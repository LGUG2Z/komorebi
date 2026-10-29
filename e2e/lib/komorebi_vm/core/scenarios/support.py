"""What more than one scenario needs of a platform."""

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
