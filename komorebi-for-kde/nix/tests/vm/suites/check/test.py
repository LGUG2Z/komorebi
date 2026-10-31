# `komorebic check` against a real (headless) KWin: what it finds of the desktop, and what it says
# when something is missing: no Plasma shell for the wallpapers, a stock effect shadowing komorebi's
# own, komorebi without its script in KWin, komorebi not running, no KWin on the bus. What only a
# real KWin can tell: that its version is read from what KWin says of itself, that the effects are
# komorebi's builds because of what they contain, that a daemon is told from its script by KWin's
# own answer, that a bus with no KWin on it is an error with an exit status that says so, and that
# the shortcut files the check compares with are in the directory `komorebic hotkeys compile`
# writes to in a session. The configuration and the exit status are covered by
# `komorebi/src/check.rs` and `komorebic/tests/check.rs`, against no desktop.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import check as kde_check

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_check.run, machine, checks, SCALE))
checks.finish()
