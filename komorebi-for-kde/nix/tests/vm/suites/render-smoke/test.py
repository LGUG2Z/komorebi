# A Plasma session draws a desktop in the test machine, so that every test after this one can trust
# a screenshot. KSplash has thousands of colors on it too, so "not blank" is not the check: the
# splash has to be gone and a window has to be painted on the desktop.

import time
from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import render_smoke as kde_render_smoke

start_all()
started = time.monotonic()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_render_smoke.run, machine, checks, SCALE))
checks.finish()
