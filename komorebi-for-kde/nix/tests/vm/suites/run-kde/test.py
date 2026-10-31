# The demo guest of `run-kde` (nix/dev/run-kde.nix), as the command starts it: the module is the
# session node's. What only a real session can tell: that the demo opens its windows and komorebi
# tiles them clear of the panel, that the cheat sheet is a window komorebi leaves alone, that what
# the cheat sheet shows of `komorebic check` has no error in it, and that the password and the key
# the host shares reach the units that use them. The host's directory is made as the guest finds it
# when it has mounted it (the node says how): another user owns it, so that the guest's own user
# cannot read what is in it, and the guest has to take it as root.
#
# The RDP checks are those of the suite `rdp`, with the password from that directory. Nothing starts
# KRDP by hand here: that it listens shows that the guest took the password before the login, as the
# command's guest has to. The command itself, `scripts/run-kde`, is checked by `run-kde-dry-run` and
# `run-kde-doctor`, which a VM cannot: it is a program of the host.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import run_kde as kde_run_kde

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_run_kde.run, machine, checks, SCALE))
checks.finish()
