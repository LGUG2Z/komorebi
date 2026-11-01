# Monitors that change, against a real headless KWin with two outputs. The shared scenario is
# `komorebi_vm.core.scenarios.hotplug`, which runs on Windows too: it unplugs a display, plugs one in
# and sends a window to it, and asks the operating system where the windows are. What only a real
# KWin can tell is in `komorebi_vm.platforms.kde.extras.hotplug`: that `kscreen-doctor` changing
# the scale of an output reaches komorebi as a change of that monitor's size, and that KWin
# reports a window on another output when komorebi sent it there. A window the user moves to the
# other output is the shared `user_move`. The shared `display_change` is skipped here: the virtual
# outputs of the headless KWin have one mode, so `KdePlatform.resizable_display` has none. `komorebi/tests/monitor_resize.rs`,
# `komorebi/tests/hotplug.rs` and `komorebi/tests/follow_output.rs` check the same against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import display_change, hotplug
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import hotplug as kde_hotplug

W, H = 1920, 1080
TITLES = ["w1", "w2", "w3"]

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), outputs=2, scale=SCALE)
platform = KdePlatform(kde, (W, H))

platform.start_komorebi()
clients = [platform.spawn(title) for title in TITLES]

run_suite(
    checks,
    platform_only("kde", kde_hotplug.setup, platform, checks, clients),
    platform_only("kde", kde_hotplug.scale, platform, checks, clients),
    platform_only("kde", kde_hotplug.sent_by_the_daemon, platform, checks, clients),
    shared(hotplug.user_move, platform, checks, clients),
    shared(hotplug.run, platform, checks, clients),
    shared(hotplug.user_move, platform, checks, clients),
    shared(display_change.run, platform, checks, clients),
    platform_only("kde", kde_hotplug.after, platform, checks, clients),
)

kde.stop_komorebi()
checks.check("komorebi stopped", not kde.komorebi_running())

checks.finish()
