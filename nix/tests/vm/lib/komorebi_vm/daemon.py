"""What the komorebi daemon logs about the events it handles.

A test reads the daemon's view of the desktop from its log
([`Kde.komorebi_log`](komorebi_vm.kde.Kde.komorebi_log)), started with
`--log-level debug`: every event it handles is an info line, every focus
change KWin reports names the window by the daemon's id and its title, and
every frame the daemon asks KWin for is a debug line.
"""

import re
from dataclasses import dataclass

from komorebi_vm.kde import Rect

_EVENT = re.compile(r"processing event: (\w+) for window (\d+) with notification (\w+)")
_ACTIVATED = re.compile(r"KWin activated window (\d+) \((.*)\)$", re.MULTILINE)
_REQUEST = re.compile(
    r"asking for window (\d+) to be at "
    r"Rect \{ left: (-?\d+), top: (-?\d+), right: (-?\d+), bottom: (-?\d+) \}"
)


@dataclass(frozen=True)
class Event:
    """A window event the daemon handled."""

    kind: str
    """The event (`Show`, `Destroy`, `FocusChange`, `Minimize`, `Restore`)."""
    window: int
    """The daemon's id for the window."""
    notification: str
    """The KWin signal it came from (`WindowAdded`, ...)."""


def events(log: str) -> list[Event]:
    """Returns the window events the daemon handled, in the order it handled them."""
    return [Event(kind, int(window), signal) for kind, window, signal in _EVENT.findall(log)]


def window_ids(log: str) -> dict[str, int]:
    """Returns the daemon's id for each window KWin has given the focus, by title.

    A title that two windows had is the later one's.
    """
    return {title: int(window) for window, title in _ACTIVATED.findall(log)}


def requests(log: str) -> dict[int, Rect]:
    """Returns the frame the daemon last asked KWin for, for each window, by the daemon's id."""
    return {
        int(window): Rect(int(x), int(y), int(w), int(h))
        for window, x, y, w, h in _REQUEST.findall(log)
    }


def asked(log: str, title: str) -> Rect | None:
    """Returns the frame the daemon last asked KWin for, for the window with this title.

    `None` if KWin never gave the window the focus (which is how its title reaches the log), or
    the daemon never asked for a frame for it.
    """
    window = window_ids(log).get(title)
    return requests(log).get(window) if window is not None else None
