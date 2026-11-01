# KWin restarts under a running daemon, as when the compositor of a session crashes or is replaced:
# the session bus stays, KWin leaves it and comes back, and the clients of the old KWin go with it.
# What only a real KWin can tell is in `komorebi_vm.platforms.kde.extras.kwin_restart`: that the
# daemon sees KWin go and come back on the bus, that it takes every window of the old KWin out of
# its layout, that it loads its script into the new KWin, and that it tiles what opens there.
# Twice, as the spike did.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import kwin_restart as kde_restart

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)
platform = KdePlatform(kde, (W, H))

run_suite(checks, platform_only("kde", kde_restart.run, platform, checks, []))

kde.stop()
checks.finish()
