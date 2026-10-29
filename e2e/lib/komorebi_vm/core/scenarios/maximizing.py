"""A maximized window has the work area, with the others left where they are behind it."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import focus_agrees, positions


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Toggles maximize on the last of `clients`, which are tiled windows of the first workspace,
    each in a container of its own, and toggles it off again."""
    target = clients[-1]
    others = [c for c in clients if c != target]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))
    area = platform.work_area()
    behind = positions(platform, others)

    def fills() -> bool:
        window = platform.os_window(target)
        return (
            window is not None
            and window.frame.w >= area.w - platform.tolerance
            and window.frame.h >= area.h - platform.tolerance
        )

    def maximized() -> str | None:
        window = platform.state().workspace().maximized
        return window.title if window else None

    platform.komorebic("toggle-maximize")
    checks.check(
        f"after 'toggle-maximize' {target.title} fills the work area",
        platform.wait_for(fills, timeout=20),
    )
    checks.check(
        "... and the daemon has it as the maximized window",
        platform.wait_for(lambda: maximized() == target.title, timeout=20),
    )
    checks.check(
        "... the others stay where they were",
        positions(platform, others) == behind,
    )

    platform.komorebic("toggle-maximize")
    checks.check(
        "toggled again, the daemon has no maximized window",
        platform.wait_for(lambda: maximized() is None, timeout=20),
    )
    checks.check(
        "... and every window is tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
