"""Where in the layout a window that opens goes, as the placement rules of the config say."""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform

CASES: list[tuple[str, Any, list[str], list[str]]] = [
    # (what it says, the placement rules, the titles in the order they open, the order they end in)
    ("no rules: after the one in focus", None, ["a", "b", "c"], ["a", "b", "c"]),
    ("every window to the primary tile", "Primary", ["a", "b", "c"], ["c", "b", "a"]),
    ("by the number of the place, counted from 1", 2, ["a", "b", "c"], ["a", "c", "b"]),
    ("text that is a number is the same place", "2", ["a", "b", "c"], ["a", "c", "b"]),
    (
        "by the title: first, last, and after the one in focus",
        {
            "Primary": {"kind": "Title", "id": "special", "matching_strategy": "Equals"},
            "Last": [{"kind": "Title", "id": "chat", "matching_strategy": "Contains"}],
        },
        ["one", "two", "special", "a chat room", "four"],
        ["special", "one", "two", "a chat room", "four"],
    ),
]


def _config(rules: Any) -> dict[str, Any]:
    workspace: dict[str, Any] = {"name": "a", "layout": "BSP"}
    if rules is not None:
        workspace["initial_window_placement_rules"] = rules
    return {
        "default_workspace_padding": 10,
        "default_container_padding": 10,
        "monitors": [{"workspaces": [workspace]}],
    }


def _order(platform: Platform) -> list[str]:
    return [titles[0] for titles in platform.state().containers()]


def _settled(platform: Platform, title: str, count: int) -> None:
    """Waits until `count` windows are managed and the last one to open has the focus."""
    platform.wait_for(lambda: platform.state().managed() == count, timeout=20)
    platform.wait_for(lambda: platform.state().focused_title() == title, timeout=20)


def run(platform: Platform, checks: Checks) -> None:
    """Starts the daemon with each case's rules, opens its windows one after the other, and checks
    the order they are in.

    Nothing else may be open: the daemon takes up every window in view, and the order would
    count it. The windows of a case are closed before the next one. A window is opened only after
    the one before is managed and has the focus, because "after the one in focus" depends on it.
    """
    for description, rules, titles, want in CASES:
        platform.restart_with(_config(rules))
        clients: list[Client] = []
        for title in titles:
            clients.append(platform.spawn(title))
            _settled(platform, title, len(clients))
        got = _order(platform)
        checks.check(f"{description}: {', '.join(want)} ({got})", got == want)
        for client in clients:
            platform.close(client)
        platform.wait_for(lambda: platform.state().managed() == 0, timeout=20)
