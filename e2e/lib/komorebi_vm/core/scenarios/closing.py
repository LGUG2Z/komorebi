"""A window that closes leaves the daemon's state, and the ones that are left share its room."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform


def run(platform: Platform, checks: Checks, clients: list[Client], closing: Client) -> None:
    """Closes `closing`, one of `clients`, which are tiled windows of the first workspace, and
    checks that the daemon let go of it and the others fill what it left.

    The windows of `clients` that are put away (on another workspace) are left out of the frames.
    """
    others = [c for c in clients if c != closing and not platform.hidden(c)]

    def area(c: Client) -> int:
        window = platform.os_window(c)
        return window.frame.w * window.frame.h if window is not None else 0

    before = sum(area(c) for c in others)
    managed = platform.state().managed()

    platform.close(closing)
    checks.check(
        f"{closing.title} is gone",
        platform.wait_for(lambda: platform.os_window(closing) is None, timeout=20),
    )
    checks.check(
        "the daemon manages one window fewer",
        platform.wait_for(lambda: platform.state().managed() == managed - 1, timeout=20),
    )
    checks.check(
        "the windows that are left take more room between them",
        platform.wait_for(lambda: sum(area(c) for c in others) > before, timeout=20),
    )
    area_of_screen = platform.work_area()
    frames = [w.frame for c in others if (w := platform.os_window(c)) is not None]
    checks.check("... inside the work area", all(f.inside(area_of_screen) for f in frames))
    checks.check(
        "... and not on each other",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
