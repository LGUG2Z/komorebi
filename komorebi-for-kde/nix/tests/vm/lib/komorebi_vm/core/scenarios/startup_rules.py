"""Workspace rules from the config: a window that was open before the daemon, and one after."""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform


def _config() -> dict[str, Any]:
    rule = {"kind": "Title", "id": "stays here", "matching_strategy": "StartsWith"}
    return {
        "monitors": [
            {
                "workspaces": [
                    {"name": "web", "layout": "BSP"},
                    {"name": "code", "layout": "BSP", "workspace_rules": [rule]},
                ]
            }
        ]
    }


def run(platform: Platform, checks: Checks) -> None:
    """Opens a window, starts the daemon with a rule that sends windows by their title to the
    second workspace, and checks where that window and one that opens afterwards are.

    The daemon must be running with no window open. The rule is in the config file, not made with
    `komorebic`, so that what is checked is how the file is read at start.
    """
    before = platform.spawn("stays here, from before")
    platform.restart_with(_config())
    checks.check(
        "a window that was open when the daemon started is on the workspace the file names",
        platform.wait_for(lambda: platform.state().workspace_of(before.title) == 1, timeout=30),
    )
    checks.check(
        "... where it is put away while the first workspace is in view",
        platform.wait_for(lambda: platform.hidden(before)),
    )
    after = platform.spawn("stays here, from after")
    checks.check(
        "a window that opens after the start goes to that workspace too",
        platform.wait_for(lambda: platform.state().workspace_of(after.title) == 1, timeout=30),
    )
    other = platform.spawn("elsewhere")
    checks.check(
        "a window no rule names stays on the first",
        platform.wait_for(lambda: platform.state().workspace_of(other.title) == 0, timeout=30),
    )
    platform.komorebic("focus-workspace", "1")
    checks.check(
        "and the windows of the rule are in view when their workspace is",
        platform.wait_for(lambda: not platform.hidden(before) and not platform.hidden(after)),
    )
    platform.komorebic("focus-workspace", "0")
    for client in (before, after, other):
        platform.close(client)
    platform.wait_for(lambda: platform.state().managed() == 0, timeout=20)
