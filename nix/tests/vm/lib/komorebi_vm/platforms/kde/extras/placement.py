"""What only KDE can tell about the placement rules of the config, beside the shared `placement`.

The rules match a window by what it is called: its Wayland app id (`Class`) and its program
(`Exe`), as well as its title. The shared scenario checks the places, by number and by title; these
check that the names a rule matches are the ones a real KWin window has, and that `foot.exe` is
`foot`.

The suite runs the shared scenario, then `by_app_id` and `by_program`, with no window open.
"""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform


def _config(rules: Any) -> dict[str, Any]:
    return {
        "default_workspace_padding": 10,
        "default_container_padding": 10,
        "monitors": [
            {
                "workspaces": [
                    {"name": "a", "layout": "BSP", "initial_window_placement_rules": rules}
                ]
            }
        ],
    }


def _order(platform: KdePlatform) -> list[str]:
    return [titles[0] for titles in platform.state().containers()]


def _open(platform: KdePlatform, opened: list[Client], title: str, app_id: str | None) -> None:
    """Opens a window, and waits until the daemon has put it in a container and given it the focus:
    "after the one in focus" depends on it."""
    opened.append(platform.spawn(title, app_id=app_id))
    platform.wait_for(lambda: platform.state().managed() == len(opened), timeout=20)
    platform.wait_for(lambda: platform.state().focused_title() == title, timeout=20)


def _close(platform: KdePlatform, opened: list[Client]) -> None:
    for client in opened:
        platform.close(client)
    platform.wait_for(lambda: platform.state().managed() == 0, timeout=20)


def by_app_id(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """A window with the app id `special` goes first, one titled `chat` goes last, and one no rule
    matches goes after the one in focus."""
    platform.restart_with(
        _config(
            {
                "Primary": {"kind": "Class", "id": "special", "matching_strategy": "Equals"},
                "Last": [{"kind": "Title", "id": "chat", "matching_strategy": "Contains"}],
            }
        )
    )
    opened: list[Client] = []
    for title in ("one", "two"):
        _open(platform, opened, title, None)
    _open(platform, opened, "three", "special")
    checks.check(
        f"the window with the app id special is first ({_order(platform)})",
        _order(platform) == ["three", "one", "two"],
    )
    _open(platform, opened, "a chat room", None)
    checks.check(
        f"the chat room is last ({_order(platform)})",
        _order(platform) == ["three", "one", "two", "a chat room"],
    )
    _open(platform, opened, "four", None)
    checks.check(
        f"a window no rule matches goes after the one in focus, the chat room ({_order(platform)})",
        _order(platform) == ["three", "one", "two", "a chat room", "four"],
    )
    _close(platform, opened)


def by_program(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """A rule for the program `foot.exe`, the name of a Windows executable, matches `foot`, which
    is what opens every window here, so every window goes to the primary tile."""
    platform.restart_with(
        _config({"Primary": {"kind": "Exe", "id": "foot.exe", "matching_strategy": "Equals"}})
    )
    opened: list[Client] = []
    for title in ("a", "b", "c"):
        _open(platform, opened, title, None)
    checks.check(
        f"foot.exe is foot, so every window is a primary one: c, b, a ({_order(platform)})",
        _order(platform) == ["c", "b", "a"],
    )
    _close(platform, opened)
