"""Where in the layout a window that opens goes, as the placement rules of the config say."""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform

CASES: list[tuple[str, Any, list[str], list[str], dict[str, str]]] = [
    # (what it says, the placement rules, the titles in the order they open, the order they end in,
    # the class of the windows that have one, by title)
    ("no rules: after the one in focus", None, ["a", "b", "c"], ["a", "b", "c"], {}),
    ("every window to the primary tile", "Primary", ["a", "b", "c"], ["c", "b", "a"], {}),
    ("by the number of the place, counted from 1", 2, ["a", "b", "c"], ["a", "c", "b"], {}),
    ("text that is a number is the same place", "2", ["a", "b", "c"], ["a", "c", "b"], {}),
    (
        "by the title: first, last, and after the one in focus",
        {
            "Primary": {"kind": "Title", "id": "special", "matching_strategy": "Equals"},
            "Last": [{"kind": "Title", "id": "chat", "matching_strategy": "Contains"}],
        },
        ["one", "two", "special", "a chat room", "four"],
        ["special", "one", "two", "a chat room", "four"],
        {},
    ),
    (
        "by the class: first, and by the title: last",
        {
            "Primary": {"kind": "Class", "id": "special-class", "matching_strategy": "Equals"},
            "Last": [{"kind": "Title", "id": "chat", "matching_strategy": "Contains"}],
        },
        ["one", "two", "three", "a chat room", "four"],
        ["three", "one", "two", "a chat room", "four"],
        {"three": "special-class"},
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


def run(platform: Platform, checks: Checks, known_failures: dict[str, str] | None = None) -> None:
    """Starts the daemon with each case's rules, opens its windows one after the other, and checks
    the order they are in. A window that has a class in the case is opened with it, for a rule that
    matches by its class.

    `known_failures` maps the description of a case to why a port is known to fail it, whether
    its daemon does not start with the rules or puts the windows in another order. The case is
    then recorded as a known failure, and the others run on.

    Nothing else may be open: the daemon takes up every window in view, and the order would
    count it. The windows of a case are closed before the next one. A window is opened only after
    the one before is managed and has the focus, because "after the one in focus" depends on it.
    """
    known = known_failures or {}
    for description, rules, titles, want, classes in CASES:
        reason = known.get(description)
        try:
            platform.restart_with(_config(rules))
        except Exception as error:
            if reason is None:
                raise
            checks.known_failure(description, False, f"{reason}; it did not start: {error}")
            continue
        clients: list[Client] = []
        for title in titles:
            clients.append(platform.spawn(title, app_id=classes.get(title)))
            _settled(platform, title, len(clients))
        got = _order(platform)
        message = f"{description}: {', '.join(want)} ({got})"
        if reason is None:
            checks.check(message, got == want)
        else:
            checks.known_failure(message, got == want, reason)
        for client in clients:
            platform.close(client)
        platform.wait_for(lambda: platform.state().managed() == 0, timeout=20)
