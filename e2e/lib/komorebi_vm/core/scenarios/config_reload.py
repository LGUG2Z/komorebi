"""The daemon reloads its `komorebi.json` when the file changes: names, layout, rules."""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform


def _config(names: list[str], layout: str, rule: str | None = None) -> dict[str, Any]:
    workspaces: list[dict[str, Any]] = [{"name": name, "layout": layout} for name in names]
    if rule is not None:
        matcher = {"kind": "Title", "id": rule, "matching_strategy": "StartsWith"}
        workspaces[1]["workspace_rules"] = [matcher]
    return {"monitors": [{"workspaces": workspaces}]}


def _names(platform: Platform) -> list[str | None]:
    return [w.name for w in platform.state().monitors[0].workspaces]


def run(platform: Platform, checks: Checks) -> None:
    """Starts the daemon from a file with two workspaces, a window open, and rewrites the file
    three times: with new names and a third workspace and another layout, with a workspace rule
    that sends the windows of a title to the second workspace, and with the rule gone.

    The daemon is left running on that file, with the window open on the first workspace, which
    is in view. The daemon must have been running, with no window but the one it is given.
    """
    stay = platform.spawn("reload-stays")
    platform.restart_with(_config(["one", "two"], "BSP"))
    checks.check(
        "the daemon starts with the workspaces of the file",
        _names(platform)[:2] == ["one", "two"],
    )
    checks.check(
        "the window that was open is managed",
        platform.wait_for(lambda: platform.state().workspace_of(stay.title) == 0, timeout=30),
    )

    platform.rewrite_config(_config(["uno", "dos", "tres"], "Columns"))
    checks.check(
        "a file with other names and one workspace more is loaded",
        platform.wait_for(lambda: _names(platform) == ["uno", "dos", "tres"], timeout=30),
    )
    checks.check(
        "... with the layout the file gives the first workspace",
        platform.wait_for(lambda: platform.state().workspace().layout == "Columns", timeout=30),
    )
    checks.check(
        "... and the window that was open stays where it was",
        platform.state().workspace_of(stay.title) == 0 and not platform.hidden(stay),
    )

    platform.rewrite_config(_config(["uno", "dos", "tres"], "Columns", rule="reload-sent"))
    sent = platform.spawn("reload-sent-1")
    checks.check(
        "a rule the file gained sends a window that opens to its workspace",
        platform.wait_for(lambda: platform.state().workspace_of(sent.title) == 1, timeout=30),
    )

    platform.rewrite_config(_config(["uno", "dos", "tres"], "Columns"))
    # the reload does not say when it is done: the next window shows whether the rule is
    platform.wait_for(lambda: False, timeout=3)
    later = platform.spawn("reload-sent-2")
    checks.check(
        "once the file loses the rule, a window of that title stays on the first workspace",
        platform.wait_for(lambda: platform.state().workspace_of(later.title) == 0, timeout=30),
    )

    platform.komorebic("change-layout", "bsp")
    for client in (sent, later):
        platform.close(client)
    checks.check(
        "the two windows are closed",
        platform.wait_for(lambda: platform.state().managed() == 1, timeout=20),
    )
