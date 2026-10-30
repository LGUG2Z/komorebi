"""The commands that take the focus, a window or a workspace to another monitor."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import os_focused
from komorebi_vm.core.snapshot import Snapshot

TITLES = ["monitors-1", "monitors-2", "monitors-3", "monitors-4"]
SECOND = "monitors-second"
"""The name of the second workspace of the second monitor, which `send-to-named-workspace` takes a
window to."""
SLACK = 2

Place = tuple[int, int]
"""A monitor and a workspace of it, by the daemon's indexes."""


def _display(platform: Platform, state: Snapshot, monitor: int) -> Rect:
    """The display the daemon's monitor is, found by where its top left corner is: the daemon may
    number its monitors in another order than the displays are, left to right."""
    size = state.monitors[monitor].size
    return min(platform.displays(), key=lambda d: abs(d.x - size.x) + abs(d.y - size.y))


def _monitor_of(platform: Platform, state: Snapshot, display: int) -> int:
    """The daemon's index of the monitor that is `displays()[display]`."""
    wanted = platform.displays()[display]
    return min(
        range(len(state.monitors)),
        key=lambda m: (
            abs(state.monitors[m].size.x - wanted.x) + abs(state.monitors[m].size.y - wanted.y)
        ),
    )


def _places(state: Snapshot, titles: list[str]) -> dict[str, Place]:
    found = {}
    for m, monitor in enumerate(state.monitors):
        for w, workspace in enumerate(monitor.workspaces):
            for window in workspace.windows():
                if window.title in titles:
                    found[window.title] = (m, w)
    return found


def _view_problem(platform: Platform, titles: list[str]) -> str | None:
    """What is wrong with the windows in view, from the operating system's side, or `None`.

    On each monitor, the windows of the workspace it shows are inside its display and not on each
    other. The windows of the workspaces it does not show are not looked at: where a platform
    keeps them is its own business.
    """
    state = platform.state()
    for m, monitor in enumerate(state.monitors):
        area = _display(platform, state, m)
        shown = monitor.workspaces[monitor.focused_workspace]
        frames = {}
        for window in shown.windows():
            if window.title not in titles:
                continue
            os_window = platform.os_window(Client(window.title))
            if os_window is None:
                return f"{window.title} has no window"
            if not os_window.frame.inside(area, SLACK):
                return f"{window.title} is not inside the display of monitor {m}"
            frames[window.title] = os_window.frame
        names = list(frames)
        for i, a in enumerate(names):
            for b in names[i + 1 :]:
                if frames[a].overlaps(frames[b]):
                    return f"{a} and {b} are on each other"
    return None


def _focus_problem(platform: Platform, monitor: int, title: str | None) -> str | None:
    """What is wrong with the focus, or `None` when the daemon and the operating system agree
    that it is on `monitor`, on the window `title` (any window that monitor shows when `None`),
    and that window is inside that monitor's display."""
    state = platform.state()
    if state.focused_monitor != monitor:
        return f"the daemon has the focus on monitor {state.focused_monitor}"
    everyone = [
        Client(w.title) for m in state.monitors for ws in m.workspaces for w in ws.windows()
    ]
    seen = os_focused(platform, everyone)
    wanted = title if title is not None else state.focused_title(monitor=monitor)
    if not seen:
        return "the operating system has no focused window"
    if seen != wanted:
        return f"the operating system has the focus on {seen} and the daemon on {wanted}"
    window = platform.os_window(Client(seen))
    if window is None or not window.frame.inside(_display(platform, state, monitor), SLACK):
        return f"{seen} has the focus but is not inside the display of monitor {monitor}"
    return None


def _after(
    platform: Platform,
    checks: Checks,
    command: str,
    expected: dict[str, Place],
    monitor: int,
    title: str | None = None,
    shown: dict[int, int] | None = None,
) -> None:
    """Checks what `command` did: each of `expected` is on its monitor and workspace, in view
    where its monitor shows it, and the focus is on `monitor`."""
    titles = list(expected)
    failed_before = checks.failed
    checks.check(
        f"after '{command}' the daemon has every window on the monitor and the workspace that "
        "were asked for, and the others where they were",
        platform.wait_for(lambda: _places(platform.state(), titles) == expected, timeout=20),
    )
    for m, workspace in (shown or {}).items():
        checks.check(
            f"after '{command}' monitor {m} shows its workspace {workspace}",
            platform.wait_for(
                lambda m=m, w=workspace: platform.state().monitors[m].focused_workspace == w
            ),
        )
    platform.wait_for(lambda: _view_problem(platform, titles) is None, timeout=20)
    problem = _view_problem(platform, titles)
    checks.check(
        f"after '{command}' the windows in view are tiled inside their own display: "
        f"{problem or 'ok'}",
        problem is None,
    )
    platform.wait_for(lambda: _focus_problem(platform, monitor, title) is None, timeout=20)
    problem = _focus_problem(platform, monitor, title)
    where = f"monitor {monitor}" + (f", on {title}" if title else "")
    checks.check(f"after '{command}' the focus is on {where}: {problem or 'ok'}", problem is None)
    if checks.failed > failed_before:
        print(_dump(platform))


def _filled(platform: Platform, monitor: int) -> int:
    """How many of a monitor's workspaces have a window."""
    return sum(1 for w in platform.state().monitors[monitor].workspaces if w.windows())


def _shows_a_workspace(platform: Platform, monitor: int) -> bool:
    """The monitor shows one of the workspaces it has."""
    state = platform.state().monitors[monitor]
    return 0 <= state.focused_workspace < len(state.workspaces)


def _dump(platform: Platform) -> str:
    """What the daemon has of each monitor: where it is, which workspace it shows, and the windows
    of each of its workspaces, for a failed check to print."""
    state = platform.state()
    lines = []
    for index, monitor in enumerate(state.monitors):
        shown = monitor.focused_workspace
        lines.append(
            f"     monitor {index} at ({monitor.size.x}, {monitor.size.y}), shows workspace {shown}"
            + (", focused" if index == state.focused_monitor else "")
        )
        for number, workspace in enumerate(monitor.workspaces):
            titles = [w.title for w in workspace.windows()]
            lines.append(f"       workspace {number} {workspace.name or ''}: {titles}")
    return "\n".join(lines)


def _focus(platform: Platform, title: str, monitor: int) -> None:
    """Gives a window the focus, and waits until the daemon has it."""
    platform.activate(Client(title))
    platform.wait_for(
        lambda: (
            platform.state().focused_monitor == monitor
            and platform.state().focused_title(monitor=monitor) == title
        ),
        timeout=20,
    )


def _plug(platform: Platform) -> bool:
    """Plugs in a display if there is only one, and returns whether it did. Waits until the daemon
    has a monitor for each display."""
    plugged = len(platform.displays()) < 2
    if plugged:
        platform.add_display()
    platform.wait_for(
        lambda: len(platform.state().monitors) == len(platform.displays()), timeout=30
    )
    return plugged


def run(platform: Platform, checks: Checks) -> None:
    """Takes windows, workspaces and the focus from one monitor to the other with each command
    that does, and checks where each window is, and where the focus is, in the daemon's state and
    on the operating system's screen.

    The daemon must be running. It may manage other windows, which stay out of the checks. The
    platform has one display or two; a second is plugged in when there is only one, and unplugged
    at the end, so that the machine has the number of displays it had. Both may have any size:
    each window is checked against the rectangle of the display its monitor is, not a constant.

    Four windows are opened and then closed. The monitors are named by the daemon's index of
    them, `a` for the one on the first display and `b` for the other, and the commands count from
    0.

    What the daemon does (`WindowManager::move_container_to_monitor` and the arms of
    `process_command`; the monitors and the workspaces count from 0), and so what is checked:

    - `send-to-monitor` takes the focused window to the target monitor's shown workspace, which
      tiles it, and leaves the focus on the monitor it left; `move-to-monitor` does the same and
      the focus follows. The `cycle-` forms take the monitor after (or before) the focused one,
      counting round: with two monitors it is always the other.
    - `send-to-monitor-workspace` and `move-to-monitor-workspace` do the same to a workspace of
      the target monitor, which that monitor then shows (it is made if it was not there); the
      named forms look the name up on every monitor.
    - `focus-monitor` and `cycle-monitor` focus the monitor, and its focused window.
      `focus-monitor-workspace` shows a workspace of a monitor and focuses it.
    - `cycle-move-workspace-to-monitor` takes the focused workspace, with its windows, to the end
      of the other monitor's workspaces, shows it there and focuses the monitor. The monitor it
      left shows a workspace it has: the one before it (kfw), or, where the monitors share their
      workspaces and one with more makes the other have as many (KDE, whose virtual desktops
      are common to the outputs), the last of its empty ones. It is done here to a workspace the
      monitor can spare: done to the only workspace a monitor has, it leaves an empty one.

    Surprising, and so not asserted: `focus-monitor` to a monitor with no window does not move the
    focus of the operating system, since there is no window to give it to, while the daemon's
    state has the monitor focused. For the same reason every window that is only sent here leaves
    another on its monitor. On a machine with one monitor the cycle forms name that monitor, and
    `cycle-send-to-monitor` puts the container at the back of its workspace. Nothing here unplugs
    a display to look at either. `focus-monitor-at-cursor` needs the pointer: see `at_cursor`.
    """
    baseline = platform.state().managed()
    clients = [platform.spawn(title) for title in TITLES]
    names = list(TITLES)
    w1, w2, w3, w4 = names
    checks.check(
        "the daemon manages the four windows",
        platform.wait_for(lambda: platform.state().managed() == baseline + 4, timeout=30),
    )
    plugged = _plug(platform)
    state = platform.state()
    a, b = _monitor_of(platform, state, 0), _monitor_of(platform, state, 1)

    # the windows that are not on the first display's first workspace are taken there
    platform.komorebic("focus-monitor", str(a))
    platform.komorebic("focus-workspace", "0")
    for title in names:
        place = _places(platform.state(), names).get(title)
        if place is not None and place != (a, 0):
            _focus(platform, title, place[0])
            platform.komorebic("move-to-monitor-workspace", str(a), "0")
    platform.komorebic("ensure-named-workspaces", str(b), "monitors-first", SECOND)
    expected: dict[str, Place] = {title: (a, 0) for title in names}
    checks.check(
        "the four windows are on the first display's first workspace",
        platform.wait_for(lambda: _places(platform.state(), names) == expected, timeout=30),
    )
    checks.check(
        "... in view and apart, on the first display",
        platform.wait_for(lambda: _view_problem(platform, names) is None, timeout=20),
    )

    # send-to-monitor: the window goes, the focus stays
    _focus(platform, w4, a)
    platform.komorebic("send-to-monitor", str(b))
    expected[w4] = (b, 0)
    _after(platform, checks, f"send-to-monitor {b}", expected, a)

    # the focus goes to a monitor and to the window it has
    platform.komorebic("focus-monitor", str(b))
    _after(platform, checks, f"focus-monitor {b}", expected, b, w4)
    platform.komorebic("focus-monitor", str(a))
    _after(platform, checks, f"focus-monitor {a}", expected, a)
    for way, monitor, title in (
        ("next", b, w4),
        ("next", a, None),
        ("previous", b, w4),
        ("previous", a, None),
    ):
        platform.komorebic("cycle-monitor", way)
        _after(platform, checks, f"cycle-monitor {way}", expected, monitor, title)

    # move-to-monitor and its cycle form: the window goes, the focus follows
    _focus(platform, w3, a)
    platform.komorebic("move-to-monitor", str(b))
    expected[w3] = (b, 0)
    _after(platform, checks, f"move-to-monitor {b}", expected, b, w3)
    platform.komorebic("cycle-move-to-monitor", "next")
    expected[w3] = (a, 0)
    _after(platform, checks, "cycle-move-to-monitor next", expected, a, w3)

    # cycle-send-to-monitor: it goes, the focus stays
    platform.komorebic("cycle-send-to-monitor", "next")
    expected[w3] = (b, 0)
    _after(platform, checks, "cycle-send-to-monitor next", expected, a)

    # a workspace of the target monitor: sent, it is shown there and the focus stays
    _focus(platform, w2, a)
    platform.komorebic("send-to-monitor-workspace", str(b), "1")
    expected[w2] = (b, 1)
    _after(platform, checks, f"send-to-monitor-workspace {b} 1", expected, a, shown={b: 1})

    # focus-monitor-workspace shows a workspace of a monitor, and focuses it
    platform.komorebic("focus-monitor-workspace", str(b), "0")
    _after(platform, checks, f"focus-monitor-workspace {b} 0", expected, b, shown={b: 0})
    platform.komorebic("focus-monitor-workspace", str(b), "1")
    _after(platform, checks, f"focus-monitor-workspace {b} 1", expected, b, w2, shown={b: 1})
    platform.komorebic("focus-monitor-workspace", str(a), "0")
    _after(platform, checks, f"focus-monitor-workspace {a} 0", expected, a, shown={a: 0})

    # move-to-monitor-workspace: the window goes, the target monitor shows its workspace, and the
    # focus follows
    platform.komorebic("focus-monitor-workspace", str(b), "1")
    platform.wait_for(lambda: _focus_problem(platform, b, w2) is None, timeout=20)
    platform.komorebic("move-to-monitor-workspace", str(a), "0")
    expected[w2] = (a, 0)
    _after(platform, checks, f"move-to-monitor-workspace {a} 0", expected, a, w2, shown={a: 0})
    _focus(platform, w1, a)
    platform.komorebic("move-to-monitor-workspace", str(b), "0")
    expected[w1] = (b, 0)
    _after(platform, checks, f"move-to-monitor-workspace {b} 0", expected, b, w1, shown={b: 0})

    # sent back to the first monitor, the focus stays on the second
    platform.komorebic("send-to-monitor", str(a))
    expected[w1] = (a, 0)
    _after(platform, checks, f"send-to-monitor {a}", expected, b)

    # a workspace by its name, which is on the second monitor: sent, it is shown, the focus stays
    _focus(platform, w1, a)
    platform.komorebic("send-to-named-workspace", SECOND)
    expected[w1] = (b, 1)
    _after(platform, checks, f"send-to-named-workspace {SECOND}", expected, a, shown={b: 1})
    _focus(platform, w2, a)
    platform.komorebic("move-to-named-workspace", SECOND)
    expected[w2] = (b, 1)
    _after(platform, checks, f"move-to-named-workspace {SECOND}", expected, b, w2, shown={b: 1})

    # a workspace to the other monitor and back: the workspace with the two windows is the one
    # the second monitor can spare. A workspace that is moved goes to the end of the monitor's
    # (the counts are read now, not earlier: on a platform whose workspaces are shared by the
    # monitors, one monitor having more makes the other have as many, empty ones)
    filled_a = _filled(platform, a)
    at_a = len(platform.state().monitors[a].workspaces)
    platform.komorebic("cycle-move-workspace-to-monitor", "next")
    expected[w1] = expected[w2] = (a, at_a)
    _after(platform, checks, "cycle-move-workspace-to-monitor next", expected, a, shown={a: at_a})
    checks.check(
        "... the monitor it left still has a workspace to show",
        platform.wait_for(lambda: _shows_a_workspace(platform, b)),
    )
    at_b = len(platform.state().monitors[b].workspaces)
    platform.komorebic("cycle-move-workspace-to-monitor", "next")
    expected[w1] = expected[w2] = (b, at_b)
    _after(
        platform,
        checks,
        "cycle-move-workspace-to-monitor next, back",
        expected,
        b,
        shown={b: at_b},
    )
    checks.check(
        "... and the first monitor has the workspaces with windows it had",
        platform.wait_for(lambda: _filled(platform, a) == filled_a),
    )

    for client in clients:
        platform.close(client)
    platform.wait_for(lambda: platform.state().managed() == baseline, timeout=30)
    platform.komorebic("focus-monitor", str(a))
    platform.komorebic("focus-workspace", "0")
    if plugged:
        platform.remove_display()
        platform.wait_for(
            lambda: len(platform.state().monitors) == len(platform.displays()), timeout=30
        )
    checks.check(
        "the daemon has the windows it had before, and a monitor for each display",
        platform.state().managed() == baseline
        and len(platform.state().monitors) == len(platform.displays()),
    )


def at_cursor(platform: Platform, checks: Checks) -> None:
    """`focus-monitor-at-cursor` focuses the monitor the pointer is on.

    Only where the platform has a pointer, which is in the pixels of the first display, so that
    the pointer can be put on the first monitor and not on the second: the focus is put on the
    second monitor with `focus-monitor`, the pointer on the first, and the command must bring the
    focus back to it.

    The daemon's state has the focus on the monitor at once. Nothing is asserted of the focus of
    the operating system, because the code does only `focus_monitor` for this message and not the
    `update_focused_workspace` that the other focus commands do: the window that has the focus on
    the screen stays the one it was until the next focus event (it differs from the daemon's
    state in the meantime).
    """
    if not platform.has("pointer"):
        checks.skip("focus-monitor-at-cursor", "the platform has no pointer")
        return
    plugged = _plug(platform)
    state = platform.state()
    a, b = _monitor_of(platform, state, 0), _monitor_of(platform, state, 1)
    first = platform.displays()[0]

    platform.komorebic("focus-monitor", str(b))
    checks.check(
        "the focus is on the second monitor",
        platform.wait_for(lambda: platform.state().focused_monitor == b),
    )
    platform.pointer.move(first.x + first.w // 2, first.y + first.h // 2)
    platform.komorebic("focus-monitor-at-cursor")
    checks.check(
        "'focus-monitor-at-cursor' with the pointer on the first display focuses the first monitor",
        platform.wait_for(lambda: platform.state().focused_monitor == a),
    )
    platform.komorebic("focus-monitor-at-cursor")
    checks.check(
        "... and again, with the focus there already, it stays",
        platform.state().focused_monitor == a,
    )
    if plugged:
        platform.remove_display()
        platform.wait_for(
            lambda: len(platform.state().monitors) == len(platform.displays()), timeout=30
        )
