# The daemon reads a real KWin through its script. What only a real KWin can tell: that the script
# loads and reports the desktop, that KWin's signals reach the window manager as the events
# `komorebi/src/kwin/state.rs` and `komorebi/tests/wm_events.rs` feed it by hand, in the order KWin
# raised them (KWin activates a new window before it announces it), that a window giving itself
# another title is reported, and that it takes its script out of KWin when it stops.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import kwin_events as kde_kwin_events

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_kwin_events.run, machine, checks, SCALE))
checks.finish()
