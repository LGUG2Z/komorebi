"""The screen is locked and unlocked: the lock screen is not tiled, nothing the daemon does while
it is locked changes the layout, and the desktop is put right when it is unlocked."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled


def _focus_agrees(platform: Platform, clients: list[Client]) -> bool:
    """The window the daemon says has the focus is the one the operating system says does."""
    workspace = platform.state().workspace()
    if not workspace.containers:
        return False
    stack = workspace.containers[workspace.focused_container]
    top = stack[workspace.focused_windows[workspace.focused_container]].title
    return any(
        (w := platform.os_window(c)) is not None and w.focused and w.title == top for c in clients
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Locks the screen with `clients` tiled windows on the first workspace, moves the first
    while it is locked, unlocks, and checks the windows are in their tiles and the focus is
    agreed on.

    The daemon must be running, with only `clients`, tiled. Needs `has("lock")`.
    """
    checks.check(
        "the windows are tiled before the lock",
        platform.wait_for(lambda: tiled(platform, clients), timeout=30),
    )
    before = [w.frame for c in clients if (w := platform.os_window(c)) is not None]

    platform.lock()
    checks.check(
        "while the screen is locked the daemon has the same windows: the lock screen is not one",
        platform.state().managed() == len(clients),
    )
    platform.move_window(clients[0], 400, 300)
    after = [platform.os_window(c) for c in clients]
    checks.check(
        "the others did not move for it",
        all(w is not None and w.frame == f for w, f in zip(after[1:], before[1:], strict=True)),
    )

    platform.unlock()
    checks.check(
        "once it is unlocked the window that was moved is in its tile again",
        platform.wait_for(lambda: tiled(platform, clients), timeout=30),
    )
    checks.check("... with the windows the daemon had", platform.state().managed() == len(clients))
    checks.check(
        "the daemon's idea of the focus is the operating system's",
        platform.wait_for(lambda: _focus_agrees(platform, clients), timeout=30),
    )
