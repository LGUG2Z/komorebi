# The screen being locked and unlocked in a real Plasma session, with komorebi running as the
# module's user unit. What only a real session can tell: that the lock screen is not tiled (it is a
# window of KWin's, on every desktop and in no task list), that nothing komorebi does while the
# screen is locked changes the layout, and that when the screen is unlocked komorebi looks at the
# desktop again and puts every window where its tile is, with its idea of the focus KWin's.
#
# A real suspend and resume is not tested: the QEMU guest has no way to wake from one. The signals
# komorebi waits for, logind's `PrepareForSleep` and the lock screen's `ActiveChanged`, are
# tested on private buses by `komorebi/tests/power.rs`, and the desktop it finds on waking against
# the fake by `komorebi/tests/wake.rs`.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import lock_sleep as kde_lock_sleep

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_lock_sleep.run, machine, checks, SCALE))
checks.finish()
