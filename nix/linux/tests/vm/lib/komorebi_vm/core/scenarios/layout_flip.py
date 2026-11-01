"""The `layout_flip` option of a workspace, in the config: the layout is mirrored from the start."""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled

CLI = {
    "Horizontal": "horizontal",
    "Vertical": "vertical",
    "HorizontalAndVertical": "horizontal-and-vertical",
}
"""The values the option takes, and how `flip-layout` spells each."""

# (layout, windows, value of the option): where a flip along an axis shows. The columns are in a
# row, so only a horizontal flip moves them; the rows are in a column, so only a vertical one does;
# BSP with three windows has one tile on a side and two stacked on the other, so both do.
CASES = [
    ("Columns", 2, "Horizontal"),
    ("BSP", 2, "Horizontal"),
    ("Columns", 3, "Horizontal"),
    ("Columns", 3, "Vertical"),
    ("Columns", 3, "HorizontalAndVertical"),
    ("Rows", 3, "Vertical"),
    ("BSP", 3, "Horizontal"),
    ("BSP", 3, "Vertical"),
    ("BSP", 3, "HorizontalAndVertical"),
]

Flip = tuple[bool, bool]
"""Whether the layout is mirrored along x (left to right) and along y (top to bottom)."""


def _flip(value: str | None) -> Flip:
    return (
        value in ("Horizontal", "HorizontalAndVertical"),
        value in ("Vertical", "HorizontalAndVertical"),
    )


def _config(layout: str, value: str | None) -> dict[str, Any]:
    workspace: dict[str, Any] = {"name": "flip", "layout": layout}
    if value is not None:
        workspace["layout_flip"] = value
    return {"monitors": [{"workspaces": [workspace]}]}


def _witnesses(layout: str, count: int) -> list[tuple[int, str]]:
    """The containers whose side of the work area says which way the layout is mirrored, each with
    the axis. Unflipped, the first column is on the left, the first row at the top, the first BSP
    tile on the left and the second at the top right."""
    return {
        "Columns": [(0, "x")],
        "Rows": [(0, "y")],
        "BSP": [(0, "x")] + ([(1, "y")] if count >= 3 else []),
    }[layout]


def _beyond_the_middle(platform: Platform, title: str, axis: str, area: Rect) -> bool | None:
    """Whether the window is on the far side of the middle of the work area (right, or below)."""
    window = platform.os_window(Client(title))
    if window is None:
        return None
    f = window.frame
    if axis == "x":
        return 2 * f.x + f.w > 2 * area.x + area.w
    return 2 * f.y + f.h > 2 * area.y + area.h


def _on_the_side(platform: Platform, layout: str, count: int, flip: Flip, area: Rect) -> bool:
    """Every witness is on the side that a layout flipped as `flip` puts it."""
    order = [c[0] for c in platform.state().containers()]
    for index, axis in _witnesses(layout, count):
        if index >= len(order):
            return False
        side = _beyond_the_middle(platform, order[index], axis, area)
        if side is None or side != flip[0 if axis == "x" else 1]:
            return False
    return True


def _centres(platform: Platform, clients: list[Client]) -> dict[str, tuple[int, int]]:
    """The middle of each window, doubled so that it is a whole number."""
    found = {}
    for client in clients:
        window = platform.os_window(client)
        if window is not None:
            f = window.frame
            found[client.title] = (2 * f.x + f.w, 2 * f.y + f.h)
    return found


def _mirrored(
    platform: Platform,
    before: dict[str, tuple[int, int]],
    now: dict[str, tuple[int, int]],
    flip: Flip,
    area: Rect,
) -> bool:
    """Every window of `now` is where the mirror of its place in `before` along `flip` is."""
    middle = (2 * area.x + area.w, 2 * area.y + area.h)
    tolerance = 2 * platform.tolerance
    return len(now) == len(before) and all(
        t in now
        and abs((2 * middle[0] - x if flip[0] else x) - now[t][0]) <= tolerance
        and abs((2 * middle[1] - y if flip[1] else y) - now[t][1]) <= tolerance
        for t, (x, y) in before.items()
    )


def _check_case(
    platform: Platform,
    checks: Checks,
    clients: list[Client],
    layout: str,
    value: str,
    compose: bool,
) -> None:
    count = len(clients)
    label = f"{layout}, {count} windows, layout_flip {value}"
    area = platform.work_area()
    flip = _flip(value)
    unflipped: Flip = (False, False)

    platform.restart_with(_config(layout, value))
    platform.wait_for(lambda: platform.state().managed() == count, timeout=30)
    checks.check(
        f"{label}: the daemon starts with the layout mirrored: the windows are where it says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=30),
    )
    checks.check(
        f"{label}: ... and on the side of the work area that the flip puts them",
        platform.wait_for(lambda: _on_the_side(platform, layout, count, flip, area), timeout=20),
    )
    mirrored = _centres(platform, clients)

    # the command and the option are one flip: the command flips it off
    platform.komorebic("flip-layout", CLI[value])
    checks.check(
        f"{label}: 'flip-layout {CLI[value]}' turns it off: the windows are on the other side",
        platform.wait_for(
            lambda: _on_the_side(platform, layout, count, unflipped, area), timeout=20
        ),
    )
    checks.check(
        f"{label}: ... each where the mirror of its place is",
        platform.wait_for(
            lambda: _mirrored(platform, mirrored, _centres(platform, clients), flip, area),
            timeout=20,
        ),
    )
    checks.check(
        f"{label}: ... and where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    platform.komorebic("flip-layout", CLI[value])
    checks.check(
        f"{label}: flipped again with the command, the windows are where the option put them",
        platform.wait_for(
            lambda: (
                _mirrored(platform, mirrored, _centres(platform, clients), unflipped, area)
                and _on_the_side(platform, layout, count, flip, area)
            ),
            timeout=20,
        ),
    )

    if compose:
        # on top of a horizontal flip, a vertical one is both
        platform.komorebic("flip-layout", "vertical")
        both = _flip("HorizontalAndVertical")
        checks.check(
            f"{label}: 'flip-layout vertical' adds to it: flipped along both axes",
            platform.wait_for(
                lambda: _on_the_side(platform, layout, count, both, area), timeout=20
            ),
        )
        checks.check(
            f"{label}: ... the windows have only gone from top to bottom",
            platform.wait_for(
                lambda: _mirrored(
                    platform, mirrored, _centres(platform, clients), (False, True), area
                ),
                timeout=20,
            ),
        )


def _left_to_right(platform: Platform, workspace: int) -> list[str] | None:
    """The titles of the containers of a workspace, ordered by where the window is, left to right;
    `None` when a window is not there."""
    order = [c[0] for c in platform.state().containers(workspace)]
    centres = _centres(platform, [Client(t) for t in order])
    if len(centres) != len(order):
        return None
    return sorted(order, key=lambda t: centres[t][0])


def _two_workspaces(platform: Platform, checks: Checks, flipped: int) -> None:
    """Two workspaces of columns, one of them flipped, each with two windows."""
    rule = {"kind": "Title", "id": "flip plain", "matching_strategy": "StartsWith"}
    workspaces: list[dict[str, Any]] = [
        {"name": "first", "layout": "Columns"},
        {"name": "second", "layout": "Columns", "workspace_rules": [rule]},
    ]
    workspaces[flipped]["layout_flip"] = "Horizontal"
    homes = [platform.spawn("flip home 1"), platform.spawn("flip home 2")]
    plains = [platform.spawn("flip plain 1"), platform.spawn("flip plain 2")]
    platform.restart_with({"monitors": [{"workspaces": workspaces}]})
    label = f"layout_flip on workspace {flipped} of two"
    checks.check(
        f"{label}: the windows are on the workspaces the rule gives them",
        platform.wait_for(
            lambda: (
                all(platform.state().workspace_of(c.title) == 0 for c in homes)
                and all(platform.state().workspace_of(c.title) == 1 for c in plains)
            ),
            timeout=30,
        ),
    )
    for workspace in (0, 1):
        platform.komorebic("focus-workspace", str(workspace))
        members = homes if workspace == 0 else plains
        platform.wait_for(lambda m=members: not any(platform.hidden(c) for c in m), timeout=20)
        order = [c[0] for c in platform.state().containers(workspace)]
        want = order[::-1] if workspace == flipped else order
        checks.check(
            f"{label}: workspace {workspace} has its windows "
            + ("mirrored" if workspace == flipped else "in the order of the layout"),
            platform.wait_for(lambda w=workspace, e=want: _left_to_right(platform, w) == e),
        )
    platform.komorebic("focus-workspace", "0")
    for client in homes + plains:
        platform.close(client)
    platform.wait_for(lambda: platform.state().managed() == 0, timeout=30)


def run(platform: Platform, checks: Checks) -> None:
    """Starts the daemon with `layout_flip` in the config of a workspace, for each value the
    option takes, and checks that the windows are mirrored as the value says, that `flip-layout`
    and the option are the one flip, and that the option is the workspace's own.

    The daemon must be running and have no window open: `restart_with` makes a daemon that takes
    up every window in view, and opening the windows before it is the usual way to have them
    there. The daemon is left running on the last config this makes.

    The option is `layout_flip` of a workspace in `komorebi.json` (`WorkspaceConfig` in
    `komorebi/src/static_config.rs`, which `Workspace::load_static_config` copies to the
    workspace, as `komorebic flip-layout` sets it). Its values are `Horizontal`, `Vertical` and
    `HorizontalAndVertical`: the layout is mirrored left to right, top to bottom, or both, which
    is what `Axis` means in `flip_layout`. `flip-layout X` along an axis the layout is already
    flipped along turns that flip off, and along another axis adds it, so that `flip-layout
    horizontal` on a layout the config flipped horizontally gives the layout back.

    Where a flip shows depends on the layout: columns are only mirrored by a horizontal flip and
    rows by a vertical one (the layouts say so), and BSP with three windows by both. So each case
    looks at the windows whose side tells: the first column, the first row, and the first and
    second tile of BSP. A flip along an axis the layout does not have changes nothing, which a
    case checks too.

    A window that was opened before the daemon is taken up by it, so the windows the cases use are
    the same ones, two and then three, and are closed at the end.
    """
    clients = [platform.spawn("layout_flip 1"), platform.spawn("layout_flip 2")]
    for layout, count, value in CASES:
        if count == 3 and len(clients) == 2:
            clients.append(platform.spawn("layout_flip 3"))
        # the flip is added to by a command in one case, which has all of the axes to show it
        compose = (layout, count, value) == ("BSP", 3, "Horizontal")
        _check_case(platform, checks, clients, layout, value, compose)
    for client in clients:
        platform.close(client)
    platform.wait_for(lambda: platform.state().managed() == 0, timeout=30)
    for flipped in (0, 1):
        _two_workspaces(platform, checks, flipped)
