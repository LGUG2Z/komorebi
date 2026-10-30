# komorebi tiles a real KWin. What only a real KWin can tell: that the commands the daemon queues
# reach KWin through the held call and that KWin carries them out, so that each window is where
# the daemon asked for it (`komorebi/tests/wm_events.rs` checks the window manager's side against
# the fake), that the daemon and KWin agree on which window has the focus, that `komorebic` reaches
# the daemon of a session and reads its state, that `komorebic focus` and `cycle-focus` move KWin's
# focus (`komorebi/tests/focus.rs` checks which window they pick), that `komorebic move` and
# `cycle-move` swap windows between their tiles (`komorebi/tests/moving.rs`), that
# `komorebic toggle-monocle` gives one window the whole work area and puts it back, and hides the
# others on the parking desktop, which a clean stop and the next start after a crash put right
# (`komorebi/tests/monocle.rs`), and that `komorebic stop` gives KWin back what the daemon set up in
# it. A window manager that agrees with itself proves nothing, so what the daemon believes is read
# from its debug log and from `komorebic state`, and what KWin has through kdotool.
from typing import cast

from komorebi_vm import Checks, Kde
from komorebi_vm.core.platform import Client
from komorebi_vm.core.scenarios import (
    closing,
    commands,
    config_reload,
    flipping,
    floating,
    focus,
    layouts,
    lifecycle,
    lifecycle_hidden,
    manage_rules,
    maximizing,
    minimizing,
    monitors,
    monocle,
    moving,
    padding,
    pausing,
    placement,
    promoting,
    resize_commands,
    rules,
    rules_class,
    rules_exe,
    scrolling,
    stacks,
    startup_rules,
    state_fidelity,
    tiling,
    tiling_off,
    workspace_moves,
    workspaces,
)
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import core_e2e as kde_core
from komorebi_vm.recorder import Recorder

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
# two outputs, of which the second is unplugged before the daemon starts: the scenarios see one
# display, as they always did, and `monitors` plugs the second in and out again
kde = Kde.headless(machine, "main", size=(W, H), outputs=2, scale=SCALE)
platform = KdePlatform(kde, (W, H))
platform.remove_display()

platform.start_komorebi()
recorder = Recorder(kde, RECORDER)
recorder.start("core-e2e")

# The shared scenarios (komorebi_vm.core.scenarios) are what runs on Windows too: they ask the
# operating system where the windows are, and not the daemon's log. After the ones that move
# windows or the focus, an extra reads the daemon's side of the bridge.
NEUTRAL = ("neutral1", "neutral2", "neutral3")
opened = run_suite(
    checks,
    platform_only("kde", kde_core.start, platform, checks, [], always=True),
    shared(tiling.run, platform, checks, NEUTRAL, always=True),
)
neutral = cast(list[Client], opened[1])
args = (platform, checks, neutral)


def after_each_shared(*steps):  # noqa: ANN002, ANN202
    """The steps, with a check after each shared one that KWin carried out what the daemon sent, so
    that a scenario that makes the bridge fail is named."""
    out = []
    for step in steps:
        out.append(step)
        if step.scope == "shared":
            out.append(platform_only("kde", kde_core.bridge_did_everything, *args))
    return out


run_suite(
    checks,
    *after_each_shared(
        platform_only("kde", kde_core.windows_where_asked, *args),
        platform_only("kde", kde_core.focus_agrees_with_the_log, *args),
        shared(focus.run, *args),
        platform_only("kde", kde_core.windows_where_asked, *args),
        shared(moving.run, *args),
        platform_only("kde", kde_core.windows_where_asked, *args),
        shared(stacks.run, *args),
        platform_only("kde", kde_core.fixtures, *args),
        shared(layouts.run, *args),
        shared(floating.run, *args),
        shared(minimizing.run, *args),
        platform_only("kde", kde_core.windows_where_asked, *args),
        shared(maximizing.run, *args),
        shared(resize_commands.run, *args),
        shared(promoting.run, *args),
        shared(flipping.run, *args),
        shared(pausing.run, *args),
        shared(tiling_off.run, *args),
        shared(scrolling.run, *args),
        shared(padding.run, *args),
        shared(monocle.run, *args),
        platform_only("kde", kde_core.windows_where_asked, *args),
        platform_only("kde", kde_core.monocle_focus_behaviour, *args),
        shared(workspace_moves.run, *args),
        shared(workspaces.run, *args),
        # the third window is on workspace 1 now: the first is closed, and the second takes its room
        shared(closing.run, *args, Client("neutral1")),
        platform_only(
            "kde", kde_core.windows_where_asked, platform, checks, neutral[1:]
        ),
        # the rules start from a workspace with no window: close what is left, wherever it is
        platform_only("kde", kde_core.no_window_is_left, platform, checks, neutral[1:]),
    ),
)

platform.komorebic("focus-workspace", "0")
nothing = (platform, checks, [])
run_suite(
    checks,
    shared(rules.run, platform, checks),
    platform_only("kde", kde_core.no_window_is_left, *nothing),
    shared(rules_class.run, platform, checks),
    shared(rules_exe.run, platform, checks),
    shared(manage_rules.run, platform, checks),
    shared(state_fidelity.run, platform, checks),
    shared(placement.run, platform, checks),
    platform_only("kde", kde_core.no_window_is_left, *nothing),
    shared(startup_rules.run, platform, checks),
    platform_only("kde", kde_core.no_window_is_left, *nothing),
    shared(config_reload.run, platform, checks),
    platform_only(
        "kde", kde_core.no_window_is_left, platform, checks, [Client("reload-stays")]
    ),
)

# a window for the stop to leave behind
platform.spawn("afterwards")
checks.check("it is managed", kde.wait_for(lambda: kde.managed_all() == 1, timeout=20))
besides = [Client("afterwards"), platform.spawn("beside")]
run_suite(
    checks,
    shared(lifecycle.run, platform, checks, besides),
    platform_only("kde", kde_core.start, platform, checks, []),
    shared(
        lifecycle_hidden.run, platform, checks, [Client("afterwards"), Client("beside")]
    ),
    platform_only("kde", kde_core.start, platform, checks, []),
)
platform.close(Client("after-the-restart"))
platform.close(Client("beside"))
checks.check(
    "one window is left", kde.wait_for(lambda: kde.managed_all() == 1, timeout=20)
)

# `move-to-monitor` and `send-to-monitor` check the monitor before they take the window out of its
# workspace (fixed: it used to be lost, as a known failure on both ports; docs/dev/BEHAVIOUR.md).
run_suite(
    checks,
    # the commands that take the focus, a window or a workspace to the other monitor: it plugs the
    # second output in, and out again
    shared(monitors.run, platform, checks),
    shared(monitors.at_cursor, platform, checks),
    shared(commands.unknown_monitor, platform, checks, [Client("afterwards")]),
    # what only KDE can tell about the daemon stopping, crashing, and KWin restarting
    platform_only("kde", kde_core.parking, platform, checks, []),
    platform_only("kde", kde_core.leave_a_window, platform, checks, []),
    platform_only("kde", kde_core.stopped, platform, checks, []),
)

recorder.stop()
kde.stop()
checks.finish()
