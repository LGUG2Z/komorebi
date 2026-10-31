# The keys of the engine survive the daemon and KWin going away. kglobalaccel keeps the keys of a
# client that died, swallowed and running nothing, until they are unregistered; and it runs inside
# KWin, so a KWin that restarts forgets them. Only a real session can say that the module's unit
# releases the keys of a daemon that was killed, that a new daemon takes them over, and that the
# daemon registers them again with the kglobalaccel of a new KWin.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import restart_survival as kde_restart_survival

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_restart_survival.run, machine, checks, SCALE))
checks.finish()
