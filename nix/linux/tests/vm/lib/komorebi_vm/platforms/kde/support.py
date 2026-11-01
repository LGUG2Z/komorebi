"""What several KDE suites and extras ask of KWin and the daemon's log, once.

Each function takes a `KdePlatform`, so a suite or an extra that needs one of them imports it from
here and does not write it again.
"""

from komorebi_vm import daemon
from komorebi_vm.core.geometry import Rect
from komorebi_vm.kde_platform import CELL, PARK, KdePlatform


def asked(platform: KdePlatform, title: str) -> Rect | None:
    """The frame the daemon last asked KWin for, for the window with this title, from its log."""
    return daemon.asked(platform.kde.komorebi_log(), title)


def where_asked(platform: KdePlatform, title: str) -> bool:
    """KWin has the window where the daemon asked: the same place, and the size up to a cell."""
    kde = platform.kde
    want, wid = asked(platform, title), kde.wid(title)
    if want is None or wid == "":
        return False
    have = kde.rect(wid)
    return (
        (have.x, have.y) == (want.x, want.y)
        and 0 <= want.w - have.w < CELL
        and 0 <= want.h - have.h < CELL
    )


def park_desktop(platform: KdePlatform) -> int:
    """The number of the parking desktop, counted from 1 as kdotool counts, or 0 for none."""
    names = platform.kde.desktop_names()
    return names.index(PARK) + 1 if PARK in names else 0


def parked(platform: KdePlatform, title: str) -> bool:
    """Whether the window is on the parking desktop, out of sight."""
    kde = platform.kde
    desk = park_desktop(platform)
    wid = kde.wid(title)
    return desk != 0 and wid != "" and kde.desk(wid) == desk
