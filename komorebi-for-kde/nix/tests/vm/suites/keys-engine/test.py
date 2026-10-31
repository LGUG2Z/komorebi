# The engine in a real Plasma session: the daemon registers the keys of a key binding file with
# KWin's global shortcut daemon and runs their commands. The Rust tests check the calls the
# engine's effects become; only KWin can say that a grabbed key reports its presses to the daemon,
# that the keys of a killed daemon stay registered until the next start takes them over, and that
# a stop gives them back.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import keys_engine as kde_keys_engine

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_keys_engine.run, machine, checks, SCALE))
checks.finish()
