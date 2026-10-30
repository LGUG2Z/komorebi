"""What only KDE can tell about the daemon and a real KWin, round the shared scenarios of
`core-e2e`.

The shared scenarios ask the operating system where the windows are and what has the focus. What
is here reads the other side of the bridge: the frame the daemon last asked KWin for, the focus
change it last handled (both from its log), the parking desktop, what KWin does when the daemon
stops or crashes, and when KWin itself restarts.
"""

import json
import re

from komorebi_vm import daemon
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import PARK, KdePlatform
from komorebi_vm.platforms.kde.support import where_asked

OUTPUT_WIDTH = 1920


def _where_asked(platform: KdePlatform, title: str) -> bool:
    return where_asked(platform, title)


def _focus_agrees(platform: KdePlatform) -> bool:
    """The window KWin has focused is the one the daemon last handled a focus change for."""
    kde = platform.kde
    log = kde.komorebi_log()
    focus_changes = [e.window for e in daemon.events(log) if e.kind == "FocusChange"]
    active = kde.active()
    if not focus_changes or active == "":
        return False
    return daemon.window_ids(log).get(kde.title(active)) == focus_changes[-1]


def _answering(platform: KdePlatform) -> bool:
    """`komorebic monitor-information` is answered, with the session's one output."""
    status, out = platform.kde.run("komorebic monitor-information")
    return status == 0 and json.loads(out)[0]["size"]["right"] == OUTPUT_WIDTH


def _parked(platform: KdePlatform, clients: list[Client]) -> list[str]:
    """The titles of these windows that are on the parking desktop, which is the last desktop."""
    kde = platform.kde
    names = kde.desktop_names()
    if PARK not in names:
        return []
    park = names.index(PARK) + 1
    return sorted(
        c.title for c in clients if kde.wid(c.title) != "" and kde.desk(kde.wid(c.title)) == park
    )


def _on_a_workspace_desktop(platform: KdePlatform, clients: list[Client]) -> bool:
    """Every one of these windows that is open is on a desktop that is a workspace."""
    kde = platform.kde
    return all(kde.desk(kde.wid(c.title)) in (1, 2) for c in clients if kde.wid(c.title) != "")


def _managed(platform: KdePlatform, clients: list[Client]) -> bool:
    state = platform.state()
    return all(state.workspace_of(c.title) is not None for c in clients)


def start(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The daemon loaded its script into KWin, which runs it, and answers komorebic."""
    kde = platform.kde
    checks.check(
        "the daemon loaded its script into KWin",
        kde.wait_for(lambda: "loaded the KWin bridge script" in kde.komorebi_log(), timeout=30),
    )
    checks.check("KWin runs the script", kde.script_loaded())
    checks.check(
        "the daemon is up and answering komorebic",
        kde.wait_for(lambda: _answering(platform), timeout=30),
    )


def windows_where_asked(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Every window is where the daemon last asked KWin for it."""
    kde = platform.kde
    checks.check(
        "every window is where the daemon asked for it",
        kde.wait_for(lambda: all(_where_asked(platform, c.title) for c in clients), timeout=20),
    )


def focus_agrees_with_the_log(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """KWin and the daemon agree on the focused window: the one the daemon last handled a focus
    change for, by its log."""
    checks.check(
        "KWin and the daemon agree on the focused window",
        platform.kde.wait_for(lambda: _focus_agrees(platform)),
    )


def fixtures(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Keeps what `komorebic state` prints as files of the run, so that the unit tests of the
    neutral core (tests/fixtures) are of a real state and not of one written from the serializer,
    and stacks the windows with `stack left` and unstacks them."""
    kde = platform.kde
    out = kde.machine.out_dir
    (out / "state-kfde-tiled.json").write_text(json.dumps(kde.state(), indent=2))
    platform.activate(clients[-1])
    kde.komorebic("stack", "left")
    checks.check(
        "the windows are stacked",
        kde.wait_for(lambda: sorted(map(len, kde.containers())) == [1, 2]),
    )
    (out / "state-kfde-stack.json").write_text(json.dumps(kde.state(), indent=2))
    kde.komorebic("unstack")
    checks.check(
        "and unstacked",
        kde.wait_for(lambda: sorted(map(len, kde.containers())) == [1, 1, 1]),
    )


# how many lines of the daemon's log `bridge_did_everything` has looked at
_LOOKED_AT: dict[int, int] = {}
_WINDOW_ID = re.compile(r'"id":"(\{[^}]+\})"')


def bridge_did_everything(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """KWin carried out every command the daemon sent it since the last time this looked: the
    daemon logs a warning when the bridge could not do one.

    A command for a window that is gone is not a failure: the daemon asks for the place of a window
    that a scenario closes a moment later, and KWin has nothing to place. Only a window that is
    still open counts.
    """
    kde = platform.kde
    lines = kde.komorebi_log().splitlines()
    start = _LOOKED_AT.get(id(platform), 0)
    _LOOKED_AT[id(platform)] = len(lines)
    alive = set(kde.run("kdotool search --class foot")[1].split())
    failed = []
    for i in range(start, len(lines)):
        if "the bridge could not do" not in lines[i]:
            continue
        ids = set(_WINDOW_ID.findall(lines[i]))
        # a window id that is not written the way the daemon writes it is not a window that is gone
        known_form = all(a.startswith("{") for a in alive)
        if not ids or ids & alive or not known_form:
            failed.append(i)
    for i in failed[:3]:
        print("     " + "\n     ".join(x[-200:] for x in lines[max(0, i - 3) : i + 1]))
    checks.check("KWin carried out every command since the last step", not failed)


def no_window_is_left(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Closes `clients` that are open, and the daemon has no window."""
    kde = platform.kde
    for client in clients:
        if kde.wid(client.title) != "":
            platform.close(client)
    checks.check("no window is left", kde.wait_for(lambda: kde.managed_all() == 0, timeout=20))


def _focus(platform: KdePlatform, client: Client, other: Client) -> None:
    """Gives the window the focus and waits until the daemon has it focused too, so that a command
    that acts on the focused window acts on this one.

    The other window is given the focus first: KWin sends no event for the focus of a window that
    already has it, and a daemon that has just started may have another one focused.
    """
    platform.activate(other)
    platform.activate(client)
    platform.kde.wait_for(
        lambda: (
            platform.state().focused_title(monitor=platform.state().focused_monitor) == client.title
        ),
        timeout=20,
    )


def _diagnose(platform: KdePlatform, clients: list[Client], what: str) -> None:
    """Prints where the daemon and KWin have the windows, for a failure to be read."""
    kde = platform.kde
    state = platform.state()
    desks = {c.title: kde.desk(kde.wid(c.title)) for c in clients if kde.wid(c.title) != ""}
    focus = state.focused_title(monitor=state.focused_monitor)
    active = kde.title(kde.active()) if kde.active() else ""
    print(f"     {what}: desktops {kde.desktop_names()}, windows on {desks}, focus {focus!r},")
    print(f"       KWin's active window is {active!r}")
    print(f"       managed {[w.title for w in state.workspace().windows()]}")


def _two_windows(platform: KdePlatform) -> list[Client]:
    clients = [platform.spawn("park1"), platform.spawn("park2")]
    platform.kde.wait_for(lambda: _managed(platform, clients), timeout=20)
    _focus(platform, clients[1], clients[0])
    return clients


def parking(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """A clean stop undoes what a monocle did, and a crash with a monocle on leaves nothing stuck
    behind: the next start removes the stale parking desktop and brings the parked window back.

    It opens two windows of its own."""
    kde = platform.kde
    two = _two_windows(platform)
    kde.komorebic("toggle-monocle")
    checks.check(
        "the window that is not in monocle is out of sight on a parking desktop",
        kde.wait_for(lambda: len(_parked(platform, two)) == 1),
    )
    _diagnose(platform, two, "monocle before the stop")
    kde.komorebic("stop")
    checks.check("the daemon exited", kde.wait_for(lambda: not kde.komorebi_running()))
    checks.check(
        "the parking desktop is gone", kde.wait_for(lambda: PARK not in kde.desktop_names())
    )
    checks.check(
        "and every window is on a workspace's desktop",
        kde.wait_for(lambda: _on_a_workspace_desktop(platform, two)),
    )

    platform.start_komorebi()
    checks.check("the daemon answers", kde.wait_for(lambda: _answering(platform), timeout=30))
    checks.check("it manages the two windows again", kde.wait_for(lambda: _managed(platform, two)))
    _focus(platform, two[1], two[0])
    kde.komorebic("toggle-monocle")
    checks.check(
        "a window is parked and a parking desktop exists",
        kde.wait_for(lambda: len(_parked(platform, two)) == 1),
    )
    _diagnose(platform, two, "monocle after the restart")
    kde.kill_komorebi()
    checks.check(
        "the daemon is dead and the parking desktop is still there, with a window on it",
        PARK in kde.desktop_names() and len(_parked(platform, two)) == 1,
    )
    platform.start_komorebi()
    checks.check(
        "the next start removed the stale parking desktop",
        kde.wait_for(lambda: _answering(platform) and PARK not in kde.desktop_names()),
    )
    checks.check(
        "and brought the parked window back",
        kde.wait_for(lambda: _on_a_workspace_desktop(platform, two)),
    )
    checks.check("it manages the two windows again", kde.wait_for(lambda: _managed(platform, two)))
    _focus(platform, two[1], two[0])
    kde.komorebic("toggle-monocle")
    checks.check(
        "after a crash and a restart a monocle still hides the other window",
        kde.wait_for(lambda: len(_parked(platform, two)) == 1),
    )
    kde.komorebic("toggle-monocle")
    checks.check(
        "and turned off, nothing is out of sight",
        kde.wait_for(lambda: _parked(platform, two) == []),
    )
    for client in two:
        platform.close(client)


def leave_a_window(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Opens the window `afterwards`, which `stopped` finds still open after the daemon stops. The
    `kwin-restart` suite covers KWin restarting under the daemon."""
    platform.spawn("afterwards")
    checks.check(
        "a window opened is managed",
        platform.kde.wait_for(lambda: platform.kde.managed_all() >= 1, timeout=20),
    )


def stopped(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """`komorebic stop` ends the daemon, removes its socket and its script, and leaves the window
    `afterwards` open."""
    kde = platform.kde
    kde.komorebic("stop")
    checks.check("the daemon exited", kde.wait_for(lambda: not kde.komorebi_running()))
    checks.check(
        "its socket is gone",
        kde.run(f"test -e {kde.runtime}/komorebi/komorebi.sock")[0] != 0,
    )
    checks.check("the script is no longer in KWin", kde.wait_for(lambda: not kde.script_loaded()))
    checks.check("the window stays", kde.wid("afterwards") != "")
