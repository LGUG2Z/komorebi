# Static shortcuts in a real Plasma session: what `komorebic hotkeys compile` and
# `komorebic export-kde-shortcuts` write is run by KWin itself, with no komorebi
# process. The Rust tests check the files; only a KWin can say that it reads them
# and runs their commands on a key: the files in the system profile when the
# session starts, and a file written later once `--reload` has handed it over.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import keys_static as kde_keys_static

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_keys_static.run, machine, checks, SCALE))
checks.finish()
