# What the harness itself promises: a headless KWin comes up with the outputs asked for, a client
# gets a window on it, and the library sees both.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import kwin as kde_kwin

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_kwin.run, machine, checks, SCALE))
checks.finish()
