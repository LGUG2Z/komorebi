"""How kfw puts windows away, and what `komorebic stop` does to them."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import WindowsPlatform


def _flags(platform: WindowsPlatform, client: Client) -> dict:
    window = platform.os_window_by_title(client.title) or {}
    return {"cloaked": window.get("cloaked"), "minimized": window.get("minimized")}


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Sends the last of `clients` to workspace 1 and minimizes the first,
    stops the daemon and starts it again. The daemon has `clients` tiled; it
    runs at the end."""
    away, minimized = clients[-1], clients[0]
    platform.activate(away)
    platform.wait_for(lambda: True, 1)
    platform.komorebic("send-to-workspace", "1")
    checks.check(
        "a window on another workspace is cloaked, not minimized",
        platform.wait_for(lambda: platform.hidden(away))
        and _flags(platform, away)["minimized"] is False,
    )
    platform.minimize(minimized)
    checks.check(
        "a minimized window is minimized, not cloaked",
        platform.wait_for(lambda: _flags(platform, minimized)["minimized"] is True)
        and _flags(platform, minimized)["cloaked"] is False,
    )

    platform.komorebic("stop")
    platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, 20)
    checks.check(
        "after `komorebic stop` the window that was put away is not cloaked",
        platform.wait_for(lambda: _flags(platform, away)["cloaked"] is False),
    )
    checks.check(
        "... and the minimized one is still minimized",
        _flags(platform, minimized)["minimized"] is True,
    )
    platform.start_komorebi()
