"""A window that floats leaves the tiling to the others, and goes back into it when it stops."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import focus_agrees


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Floats the last of `clients`, which are tiled windows of the first workspace in a
    container each, with more than one of them, and then tiles it again."""
    target = clients[-1]
    others = clients[:-1]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))

    def area(c: Client) -> int:
        window = platform.os_window(c)
        return window.frame.w * window.frame.h if window is not None else 0

    def floating() -> list[str]:
        return [w.title for w in platform.state().workspace().floating]

    before = sum(area(c) for c in others)
    managed = platform.state().managed()

    platform.komorebic("toggle-float")
    checks.check(
        f"after 'toggle-float' the daemon has {target.title} floating",
        platform.wait_for(lambda: floating() == [target.title], timeout=20),
    )
    checks.check("... and still manages every window", platform.state().managed() == managed)
    checks.check(
        "... the others take more room between them",
        platform.wait_for(lambda: sum(area(c) for c in others) > before, timeout=20),
    )
    frames = [w.frame for c in others if (w := platform.os_window(c)) is not None]
    checks.check(
        "... inside the work area and not on each other",
        all(f.inside(platform.work_area()) for f in frames)
        and not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
    checks.check(
        f"... {target.title} is still there, and not put away",
        platform.os_window(target) is not None and not platform.hidden(target),
    )

    platform.komorebic("toggle-float")
    checks.check(
        f"after another 'toggle-float' {target.title} floats no more",
        platform.wait_for(lambda: floating() == [], timeout=20),
    )
    checks.check(
        "... and every window is tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
