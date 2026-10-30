"""The resize commands move an edge of the focused window, with no pointer."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees

# how much wider the window must get for the command to have done something
MIN_STEP = 10


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Resizes the first of `clients`, which are tiled windows of the first workspace, with
    `resize-axis horizontal increase` and `decrease`."""
    target = clients[0]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))

    def width() -> int:
        window = platform.os_window(target)
        return window.frame.w if window is not None else 0

    def clear() -> bool:
        frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
        area = platform.work_area()
        return all(f.inside(area) for f in frames) and not any(
            a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]
        )

    before = width()
    platform.komorebic("resize-axis", "horizontal", "increase")
    checks.check(
        f"after 'resize-axis horizontal increase' {target.title} is wider",
        platform.wait_for(lambda: width() >= before + MIN_STEP, timeout=20),
    )
    print(f"     width {before} -> {width()}")
    checks.check("... and every window is inside the work area, none over another", clear())

    platform.komorebic("resize-axis", "horizontal", "decrease")
    checks.check(
        "after 'decrease' it has the width it had",
        platform.wait_for(lambda: abs(width() - before) <= platform.tolerance, timeout=20),
    )
    checks.check("... and every window is inside the work area, none over another", clear())
