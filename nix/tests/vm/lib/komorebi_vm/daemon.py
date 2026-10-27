"""What the komorebi daemon logs about the events it handles.

A test reads the daemon's view of the desktop from its log
([`Kde.komorebi_log`](komorebi_vm.kde.Kde.komorebi_log)), started with
`--log-level debug`: every event it handles is an info line, and every focus
change KWin reports names the window by the daemon's id and its title.
"""

import re
from dataclasses import dataclass

_EVENT = re.compile(r"processing event: (\w+) for window (\d+) with notification (\w+)")
_ACTIVATED = re.compile(r"KWin activated window (\d+) \((.*)\)$", re.MULTILINE)


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
