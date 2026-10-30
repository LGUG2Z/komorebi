# A window the user maximizes, in a real headless KWin. The user's title bar maximizes a window
# without asking komorebi: KWin does it and reports it, and komorebi has to take the window out of
# the layout as it does for `toggle-maximize`, leave the layout alone while the window is maximized,
# and put the window back in its tile when it is restored, by the user or by komorebi.
# `komorebi/tests/user_maximize.rs` checks what the window manager does against the fake; this
# checks that KWin reports the user's maximize, and the restore, the way komorebi reads them.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import user_maximize as kde_user_maximize

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_user_maximize.run, machine, checks, SCALE))
checks.finish()
