"""A window that is minimized leaves its room to the others, and takes it back when restored."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Minimizes the first of `clients`, which are tiled windows of the first workspace in a
    container each, with more than two of them, and restores it."""
    target = clients[0]
    others = clients[1:]

    def area(c: Client) -> int:
        window = platform.os_window(c)
        return window.frame.w * window.frame.h if window is not None else 0

    before = sum(area(c) for c in others)
    platform.minimize(target)
    checks.check(
        f"with {target.title} minimized the others take more room between them",
        platform.wait_for(lambda: sum(area(c) for c in others) > before, timeout=20),
    )
    frames = [w.frame for c in others if (w := platform.os_window(c)) is not None]
    checks.check(
        "... inside the work area and not on each other",
        all(f.inside(platform.work_area()) for f in frames)
        and not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )

    platform.activate(target)
    checks.check(
        f"restored, {target.title} and the others are tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    checks.check(
        "... and not on each other",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
