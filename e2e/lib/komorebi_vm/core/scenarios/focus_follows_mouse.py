"""With focus follows mouse on, the window under the pointer has the focus; with it off, not."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform


def _middle(platform: Platform, client: Client) -> tuple[int, int] | None:
    window = platform.os_window(client)
    if window is None:
        return None
    return window.frame.x + window.frame.w // 2, window.frame.y + window.frame.h // 2


def _focused(platform: Platform, client: Client) -> bool:
    window = platform.os_window(client)
    return window is not None and window.focused


def _settle(platform: Platform) -> None:
    """Gives a platform the time to take up a setting: KWin reads its focus policy about 200 ms
    after it is written, and a pointer that is moved before that is moved under the old one."""
    platform.wait_for(lambda: False, timeout=1)


def run(platform: Platform, checks: Checks, first: Client, second: Client) -> None:
    """Turns focus follows mouse on, moves the pointer over `first` and `second` in turn, turns it
    off, and on again, and checks where the focus is.

    `first` and `second` are two tiled windows in view, not on each other. The setting is left off
    at the end. The platform needs a pointer; without one the scenario says so and stops.
    """
    if not platform.has("pointer"):
        print("     skipped: this platform has no pointer")
        return
    a, b = _middle(platform, first), _middle(platform, second)
    checks.check("both windows have a middle for the pointer", a is not None and b is not None)
    if a is None or b is None:
        return

    platform.komorebic("focus-follows-mouse", "enable")
    _settle(platform)
    platform.pointer.move(*a)
    checks.check(
        "the pointer over the first window gives it the focus",
        platform.wait_for(lambda: _focused(platform, first), timeout=10),
    )
    platform.pointer.move(*b)
    checks.check(
        "over the second, the second",
        platform.wait_for(lambda: _focused(platform, second), timeout=10),
    )
    platform.pointer.move(*a)
    checks.check("and back", platform.wait_for(lambda: _focused(platform, first), timeout=10))
    checks.check(
        "the daemon follows: it has the first window focused",
        platform.wait_for(lambda: platform.state().focused_title() == first.title, timeout=10),
    )

    platform.komorebic("toggle-focus-follows-mouse", "--implementation", "windows")
    _settle(platform)
    platform.pointer.move(*b)
    checks.check(
        "turned off, the pointer over the second window no longer moves the focus",
        not platform.wait_for(lambda: _focused(platform, second), timeout=3)
        and _focused(platform, first),
    )

    platform.komorebic("focus-follows-mouse", "enable")
    _settle(platform)
    # the pointer rests over the second window: it has to go into a window to be followed. The
    # first window already has the focus, so there is nothing to wait for there: the pointer rests
    # over it for a moment, as a person's would, and a platform that follows the pointer by a
    # hover delay (Windows) has seen it enter that window before it enters the other
    platform.pointer.move(*a)
    _settle(platform)
    platform.pointer.move(*b)
    checks.check(
        "on again, the pointer over the second window gives it the focus",
        platform.wait_for(lambda: _focused(platform, second), timeout=10),
    )
    platform.komorebic("focus-follows-mouse", "disable")
