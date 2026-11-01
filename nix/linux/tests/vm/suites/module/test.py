# The NixOS module runs komorebi in a real Plasma session: nothing in this test starts the daemon.
# What only a real session can tell: that logging in starts the module's user unit through
# `plasma-workspace.target`, that the daemon it runs reaches KWin from there and tiles a window
# opened in the session, that it runs with the module's log level, the `komorebi.json` the module
# wrote from its `config`, and the PATH of the user's session, that systemd starts it again when
# it is killed, and that a clean stop is not a failure. Once the unit is stopped, the daemon is
# started with a `komorebi.json` of the user's, which it follows when the file changes, and
# `komorebic enable-autostart` writes the entry systemd's autostart generator makes a unit of. The
# options and the unit the module writes are checked without a machine by `nixos-module-eval`
# (`nix/tests/nixos-module.nix`).

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import module as kde_module

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_module.run, machine, checks, SCALE))
checks.finish()
