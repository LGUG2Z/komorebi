"""Every layout, and a sweep of komorebic's commands: none may break the daemon.

After each command the same things have to be true: the daemon is alive, it answers `komorebic
state`, and it manages every window the operating system has. A command is also expected to be
understood (exit status 0). A command the port does not have (`error: unrecognized subcommand`) is
skipped, with its name, which is what the behaviour matrix of the two ports is made of. What each
command does is checked elsewhere; this says none of them breaks a running daemon.
"""

from collections.abc import Sequence

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform

LAYOUTS: tuple[str, ...] = (
    "bsp",
    "columns",
    "rows",
    "vertical-stack",
    "horizontal-stack",
    "ultrawide-vertical-stack",
    "grid",
    "right-main-vertical-stack",
    "scrolling",
)

# A command with an option that has a default is given the option here when one port requires some
# argument: kfw's `toggle-focus-follows-mouse` prints its help and exits 2 without one, and kfde's
# accepts it.
#
# Every command the ports share that changes something, in an order that leaves the daemon with
# windows to act on. `{config}` is the path of a configuration file for `replace-configuration`.
SWEEP: tuple[str, ...] = (
    "focus left",
    "focus right",
    "focus up",
    "focus down",
    "move left",
    "move right",
    "move up",
    "move down",
    "cycle-focus next",
    "cycle-focus previous",
    "cycle-move next",
    "cycle-move previous",
    "stack left",
    "cycle-stack next",
    "cycle-stack previous",
    "cycle-stack-index next",
    "focus-stack-window 0",
    "unstack",
    "stack-all",
    "unstack-all",
    "resize-edge left increase",
    "resize-edge right decrease",
    "resize-edge up increase",
    "resize-edge down decrease",
    "resize-axis horizontal increase",
    "resize-axis vertical decrease",
    "resize-delta 50",
    "resize-delta 100",
    "promote",
    "promote-swap",
    "promote-focus",
    "promote-window left",
    "retile",
    "flip-layout horizontal",
    "flip-layout vertical",
    "flip-layout horizontal-and-vertical",
    "flip-layout horizontal-and-vertical",
    "cycle-layout next",
    "cycle-layout previous",
    "layout-ratios --columns 0.6 0.4",
    "change-layout columns",
    "scrolling-layout-columns 2",
    "change-layout bsp",
    "preselect-direction left",
    "cancel-preselect",
    "toggle-monocle",
    "toggle-monocle",
    "toggle-maximize",
    "toggle-maximize",
    "toggle-float",
    "toggle-float",
    "toggle-lock",
    "toggle-lock",
    "toggle-tiling",
    "toggle-tiling",
    "toggle-pause",
    "toggle-pause",
    "toggle-workspace-layer",
    "toggle-workspace-layer",
    "toggle-window-container-behaviour",
    "toggle-window-container-behaviour",
    "toggle-float-override",
    "toggle-float-override",
    "toggle-workspace-window-container-behaviour",
    "toggle-workspace-window-container-behaviour",
    "toggle-workspace-float-override",
    "toggle-workspace-float-override",
    "ensure-named-workspaces 0 one two three",
    "workspace-name 0 0 one",
    "ensure-workspaces 0 3",
    "workspace-tiling 0 0 disable",
    "workspace-tiling 0 0 enable",
    "named-workspace-tiling one disable",
    "named-workspace-tiling one enable",
    "focus-workspace 1",
    "focus-workspace 0",
    "focus-last-workspace",
    "focus-last-workspace",
    "focus-workspaces 1",
    "focus-workspaces 0",
    "cycle-workspace next",
    "cycle-workspace previous",
    "cycle-empty-workspace next",
    "focus-workspace 0",
    "focus-named-workspace two",
    "focus-named-workspace one",
    "send-to-workspace 1",
    "focus-workspace 1",
    "move-to-workspace 0",
    "focus-workspace 0",
    "cycle-send-to-workspace next",
    "focus-workspace 1",
    "cycle-move-to-workspace previous",
    "focus-workspace 0",
    "send-to-last-workspace",
    "move-to-last-workspace",
    "focus-workspace 0",
    "new-workspace",
    "close-workspace",
    "focus-workspace 0",
    "workspace-layout 0 0 columns",
    "workspace-layout 0 0 bsp",
    "named-workspace-layout one columns",
    "named-workspace-layout one bsp",
    "workspace-layout-rule 0 0 2 rows",
    "workspace-layout-rule 0 0 3 columns",
    "clear-workspace-layout-rules 0 0",
    "named-workspace-layout-rule one 3 columns",
    "clear-named-workspace-layout-rules one",
    "workspace-layout 0 0 bsp",
    "focus-monitor 1",
    "focus-monitor 0",
    "cycle-monitor next",
    "cycle-monitor previous",
    "focus-monitor-workspace 1 1",
    "focus-monitor-workspace 0 0",
    "focus-monitor-at-cursor",
    "focus-monitor 0",
    "send-to-named-workspace two",
    "focus-named-workspace two",
    "move-to-named-workspace one",
    "focus-named-workspace one",
    "send-to-monitor 1",
    "focus-monitor 1",
    "move-to-monitor 0",
    "focus-monitor 0",
    "cycle-send-to-monitor next",
    "focus-monitor 1",
    "cycle-move-to-monitor next",
    "focus-monitor 0",
    "send-to-monitor-workspace 1 1",
    "focus-monitor-workspace 1 1",
    "move-to-monitor-workspace 0 0",
    "focus-monitor 0",
    "focus-workspace 0",
    "move-workspace-to-monitor 1",
    "cycle-move-workspace-to-monitor next",
    "swap-workspaces-with-monitor 1",
    "swap-workspaces-with-monitor 1",
    "global-work-area-offset 10 10 20 20",
    "global-work-area-offset 0 0 0 0",
    "monitor-work-area-offset 0 10 10 20 20",
    "monitor-work-area-offset 0 0 0 0 0",
    "workspace-work-area-offset 0 0 10 10 20 20",
    "workspace-work-area-offset 0 0 0 0 0 0",
    "toggle-window-based-work-area-offset",
    "toggle-window-based-work-area-offset",
    "focused-workspace-container-padding 20",
    "focused-workspace-container-padding 10",
    "focused-workspace-padding 20",
    "focused-workspace-padding 10",
    "adjust-container-padding increase 5",
    "adjust-container-padding decrease 5",
    "adjust-workspace-padding increase 5",
    "adjust-workspace-padding decrease 5",
    "container-padding 0 0 12",
    "container-padding 0 0 10",
    "workspace-padding 0 0 12",
    "workspace-padding 0 0 10",
    "named-workspace-container-padding one 12",
    "named-workspace-container-padding one 10",
    "named-workspace-padding one 12",
    "named-workspace-padding one 10",
    "cross-monitor-move-behaviour insert",
    "cross-monitor-move-behaviour swap",
    "toggle-cross-monitor-move-behaviour",
    "toggle-cross-monitor-move-behaviour",
    "monocle-focus-behaviour cycle",
    "toggle-monocle-focus-behaviour",
    "toggle-monocle-focus-behaviour",
    "unmanaged-window-operation-behaviour op",
    "unmanaged-window-operation-behaviour no-op",
    "unmanaged-window-operation-behaviour op",
    "display-index-preference 0 Virtual-0",
    "mouse-follows-focus enable",
    "mouse-follows-focus disable",
    "toggle-mouse-follows-focus",
    "toggle-mouse-follows-focus",
    "focus-follows-mouse enable",
    "focus-follows-mouse disable",
    "toggle-focus-follows-mouse --implementation windows",
    "toggle-focus-follows-mouse --implementation windows",
    "transparency enable",
    "transparency-alpha 150",
    "toggle-transparency",
    "toggle-transparency",
    "transparency disable",
    "border enable",
    "border-width 8",
    "border-offset 1",
    "border-colour --window-kind single 10 20 30",
    "border disable",
    "animation enable",
    "animation-duration 120",
    "animation-fps 30",
    "animation-style --style linear",
    "animation disable",
    "session-float-rule",
    "session-float-rules",
    "clear-session-float-rules",
    "toggle-float",
    "ignore-rule exe nonexistent",
    "manage-rule exe nonexistent",
    "initial-workspace-rule exe nonexistent 0 1",
    "workspace-rule exe nonexistent 0 1",
    "initial-named-workspace-rule exe nonexistent two",
    "named-workspace-rule exe nonexistent two",
    "enforce-workspace-rules",
    "clear-workspace-rules 0 1",
    "clear-named-workspace-rules two",
    "clear-all-workspace-rules",
    "manage",
    "close",
    "retile",
    "replace-configuration {config}",
    "eager-focus foot",
)

# What komorebic only prints.
PRINTED: tuple[str, ...] = (
    "state",
    "global-state",
    "visible-windows",
    "monitor-information",
    "data-directory",
    "configuration",
    "generate-static-config",
    "static-config-schema",
    "application-specific-configuration-schema",
    "notification-schema",
    "socket-schema",
    "session-float-rules",
)


NOT_A_COMMAND = "unrecognized subcommand"


def unhealthy(platform: Platform, clients: list[Client]) -> str | None:
    """Why the daemon is not healthy, or `None`: it answers `state` and manages the windows the
    operating system has."""
    status, _ = platform.komorebic_status("state")
    if status != 0:
        return "it does not answer `komorebic state`"
    try:
        managed = platform.state().managed()
    except (AssertionError, ValueError, KeyError) as error:
        return f"its state cannot be read ({error})"
    present = sum(1 for c in clients if platform.os_window(c) is not None)
    if managed != present:
        return f"it manages {managed} windows and the operating system has {present}"
    return None


def layouts(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Changes to every layout, and checks that the windows stay inside the work area and apart
    from each other, and that the daemon stays healthy. The scrolling layout is a strip whose
    columns can be past the edge, so its frames are not looked at."""
    area = platform.work_area()
    for layout in LAYOUTS:
        status, out = platform.komorebic_status("change-layout", layout)
        if status != 0 and NOT_A_COMMAND in out:
            checks.skip(f"change-layout {layout}", "the port has no such layout")
            continue
        problems = []
        if status != 0:
            problems.append(f"exit {status}")
        platform.wait_for(lambda: unhealthy(platform, clients) is None, timeout=10)
        if (why := unhealthy(platform, clients)) is not None:
            problems.append(why)
        if layout != "scrolling":
            frames = {c.title: w.frame for c in clients if (w := platform.os_window(c)) is not None}
            problems += [
                f"{t} is outside the work area" for t, f in frames.items() if not f.inside(area, 2)
            ]
            titles = list(frames)
            problems += [
                f"{a} and {b} overlap"
                for i, a in enumerate(titles)
                for b in titles[i + 1 :]
                if frames[a].overlaps(frames[b])
            ]
        checks.check(f"change-layout {layout}: " + ("; ".join(problems) or "ok"), not problems)


def sweep(
    platform: Platform,
    checks: Checks,
    clients: list[Client],
    config: str,
    commands: Sequence[str] = SWEEP,
) -> None:
    """Runs every command of `commands` (`SWEEP`), and checks that it is understood and leaves the
    daemon healthy. `config` is the path, on the platform, of a file the daemon can be told to
    load.

    The sweep has commands that name the second monitor (`send-to-monitor 1`), so it needs two
    displays: one is plugged in first if the platform has only one.
    """
    if len(platform.displays()) < 2:
        platform.add_display()
        platform.wait_for(lambda: len(platform.state().monitors) >= 2, timeout=30)
    for line in commands:
        words = line.format(config=config).split()
        status, out = platform.komorebic_status(*words)
        if status != 0 and NOT_A_COMMAND in out:
            checks.skip(f"komorebic {line}", "the port has no such command")
            continue
        if status != 0:
            checks.bad(f"komorebic {line}: exit {status}: {' '.join(out.splitlines()[:2])}")
            continue
        platform.wait_for(lambda: unhealthy(platform, clients) is None, timeout=5)
        if (why := unhealthy(platform, clients)) is not None:
            checks.bad(f"komorebic {line}: afterwards {why}")
            continue
        checks.ok(f"komorebic {line}")


def prints(platform: Platform, checks: Checks) -> None:
    """Runs every command that only prints something, and checks that each exits 0."""
    for line in PRINTED:
        status, out = platform.komorebic_status(*line.split())
        if status != 0 and NOT_A_COMMAND in out:
            checks.skip(f"komorebic {line}", "the port has no such command")
            continue
        checks.check(f"komorebic {line}", status == 0)


def unknown_monitor(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Sends the focused window to a monitor that does not exist, and checks that no window is lost.

    A port may refuse the command or ignore it, but a window that was managed has to stay managed:
    a daemon that stops managing it leaves it on the screen with nothing tiling it. The platform
    has fewer than seven displays.
    """
    managed = platform.state().managed()
    for command in ("send-to-monitor", "move-to-monitor"):
        platform.komorebic_status(command, "7")
        platform.wait_for(lambda: unhealthy(platform, clients) is None, timeout=5)
        checks.check(
            f"after {command} 7, which no display has, the daemon still manages every window",
            platform.state().managed() == managed and unhealthy(platform, clients) is None,
        )
