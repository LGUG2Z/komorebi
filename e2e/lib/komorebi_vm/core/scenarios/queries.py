"""`komorebic query` answers with what the daemon's state says, and the resize a workspace has can
be saved and loaded."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Platform
from komorebi_vm.core.scenarios.commands import NOT_A_COMMAND
from komorebi_vm.core.snapshot import Snapshot

# the layouts that `change-layout` takes by a name that is the lower case of the one a query gives
BY_NAME = {"BSP": "bsp", "Columns": "columns", "Rows": "rows"}


def _expected(state: Snapshot) -> dict[str, str]:
    """What each query that depends on the focus should answer, from the daemon's own state."""
    monitor = state.monitors[state.focused_monitor]
    workspace = state.workspace(monitor=state.focused_monitor)
    expected = {
        "focused-monitor-index": str(state.focused_monitor),
        "focused-workspace-index": str(monitor.focused_workspace),
        "focused-workspace-name": workspace.name or str(monitor.focused_workspace),
        "focused-workspace-layout": workspace.layout,
    }
    if workspace.containers:
        container = workspace.containers[workspace.focused_container]
        expected["focused-container-index"] = str(workspace.focused_container)
        expected["focused-window-index"] = str(
            workspace.focused_windows[workspace.focused_container]
        )
        expected["focused-container-kind"] = "Stack" if len(container) > 1 else "Single"
    return expected


def _query(platform: Platform, checks: Checks, state: Snapshot) -> None:
    for query, want in _expected(state).items():
        status, out = platform.komorebic_status("query", query)
        if status != 0 and NOT_A_COMMAND in out:
            checks.skip(f"query {query}", "the port has no such query")
            continue
        checks.check(f"query {query} answers {want!r}", status == 0 and out.strip() == want)


def _layout(platform: Platform) -> list[Rect]:
    return platform.state().workspace().latest_layout


def _round_trip(
    platform: Platform, checks: Checks, name: str, save: tuple[str, ...], load: tuple[str, ...]
) -> None:
    """Saves the resize, resizes the focused container, loads it, and checks that the layout is
    the one it was."""
    before = _layout(platform)
    status, _ = platform.komorebic_status(*save)
    platform.komorebic("resize-axis", "horizontal", "increase")
    resized = platform.wait_for(lambda: _layout(platform) != before, timeout=10)
    status_load, _ = platform.komorebic_status(*load)
    checks.check(
        f"{name}: a resize is undone by loading what was saved",
        status == 0
        and resized
        and status_load == 0
        and platform.wait_for(lambda: _layout(platform) == before, timeout=10),
    )


def run(platform: Platform, checks: Checks) -> None:
    """Asks every query and checks the answer against the daemon's state, after changing the
    focus and the layout so that the answers are not the ones of a fresh daemon, and saves and
    loads the resize both ways.

    The focused workspace has two containers or more. The focus and the layout are as they were at
    the end.
    """
    _query(platform, checks, platform.state())
    status, out = platform.komorebic_status("query", "version")
    checks.check("query version answers", status == 0 and out.strip() != "")

    workspace = platform.state().workspace()
    if len(workspace.containers) < 2:
        checks.skip("queries after a change, and the saved resize", "fewer than two containers")
        return

    platform.komorebic("cycle-focus", "next")
    platform.wait_for(
        lambda: platform.state().workspace().focused_container != workspace.focused_container,
        timeout=10,
    )
    _query(platform, checks, platform.state())
    platform.komorebic("cycle-focus", "previous")

    if workspace.layout in BY_NAME:
        other = "columns" if workspace.layout == "Rows" else "rows"
        platform.komorebic("change-layout", other)
        platform.wait_for(lambda: platform.state().workspace().layout != workspace.layout)
        _query(platform, checks, platform.state())
        platform.komorebic("change-layout", BY_NAME[workspace.layout])
        platform.wait_for(lambda: platform.state().workspace().layout == workspace.layout)

    _round_trip(
        platform, checks, "quick-save-resize", ("quick-save-resize",), ("quick-load-resize",)
    )
    path = platform.temp_path("e2e-resize.json")
    _round_trip(platform, checks, "save-resize", ("save-resize", path), ("load-resize", path))
