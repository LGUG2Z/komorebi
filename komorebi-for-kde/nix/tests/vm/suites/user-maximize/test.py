# A window the user maximizes, in a real headless KWin. The user's title bar maximizes a window
# without asking komorebi: KWin does it and reports it, and komorebi has to take the window out of
# the layout as it does for `toggle-maximize`, leave the layout alone while the window is maximized,
# and put the window back in its tile when it is restored, by the user or by komorebi.
# `komorebi/tests/user_maximize.rs` checks what the window manager does against the fake. The shared
# scenario `komorebi_vm.core.scenarios.maximizing.user` makes those checks through the neutral
# platform; `komorebi_vm.platforms.kde.extras.user_maximize` starts the daemon and reads the
# daemon's log for what KWin reported.

from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import maximizing
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import user_maximize as kde_user_maximize

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)
platform = KdePlatform(kde, (W, H))
clients = kde_user_maximize.start(platform, checks)

run_suite(
    checks,
    shared(maximizing.user, platform, checks, clients),
    platform_only("kde", kde_user_maximize.log, platform, checks, clients),
)

kde.komorebic("stop")
kde.wait_for(lambda: not kde.komorebi_running())
kde.stop()
checks.finish()
