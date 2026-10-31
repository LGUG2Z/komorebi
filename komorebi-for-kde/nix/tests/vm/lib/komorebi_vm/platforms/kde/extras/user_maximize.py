r"""A window the user maximizes, in a real headless KWin. The user's title bar maximizes a window
without asking komorebi: KWin does it and reports it, and komorebi has to take the window out of
the layout as it does for `toggle-maximize`, leave the layout alone while the window is maximized,
and put the window back in its tile when it is restored, by the user or by komorebi.
`komorebi/tests/user_maximize.rs` checks what the window manager does against the fake.

The shared `maximizing.user` makes those checks through the neutral platform. What is here is the
start (`start`: a daemon with padding and no animation, and three tiled windows) and what only the
daemon's log of this port says (`log`): that KWin's report was read as a change of the window's
maximized state, and that what was asked of the layout was noted as ignored.
"""

from komorebi_vm import daemon
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform

# The windows are tiled with a gap of 10 pixels around the workspace and between the tiles: a tile
# has padding, and a maximized window has none. The windows are not animated.
CONFIG = {
    "default_workspace_padding": 10,
    "default_container_padding": 10,
    "animation": {"enabled": False},
}


def start(platform: KdePlatform, checks: Checks) -> list[Client]:
    """Starts the daemon with the config, opens three windows and waits for them to be tiled."""
    platform.restart_with(CONFIG)
    clients = [platform.spawn(title) for title in ("um-a", "um-b", "um-c")]
    checks.check(
        "komorebi has three tiles",
        platform.wait_for(
            lambda: len(platform.state().workspace().containers) == len(clients), timeout=20
        ),
    )
    # The bridge asks again for the position of a window that is not where komorebi put it for a
    # second after it did (PLATFORM.md, "How clients answer a frame"), which would put a window that
    # is maximized in that second back where its tile was: wait that second out, as a person would.
    platform.wait_for(lambda: platform.state().focused_title() == "um-c", timeout=20)
    platform.wait_for(lambda: False, timeout=3)
    return clients


def log(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The daemon's log says it handled KWin's report as a change of the maximized state, and
    that it ignored what was asked of the layout meanwhile."""
    text = platform.kde.komorebi_log()
    checks.check(
        "komorebi handled it as a change of the window's maximized state",
        any(
            event.kind == "MaximizeChange" and event.notification == "WindowMaximizedChanged"
            for event in daemon.events(text)
        ),
    )
    checks.check(
        "komorebi noted that it ignored what was asked of the layout",
        "ignoring" in text and "the window in front is maximized" in text,
    )
