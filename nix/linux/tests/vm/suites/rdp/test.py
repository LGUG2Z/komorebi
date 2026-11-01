# KRDP, Plasma's RDP server, runs in the Plasma session of the session node as the guest to develop
# in runs it (nix/dev/krdp.nix), and a client logs in and gets a picture of the desktop. What only a
# real session can tell: that the unit starts in the session once it has a password, that KWin lets
# KRDP have the screen without asking, that a client with the password is let in and one without
# it is not, and that the picture the client gets is what the display shows.
#
# The client is FreeRDP's, on an X server of its own (`komorebi_vm.RdpClient`), so its picture can be
# read back and compared with a screenshot of the display. The suite `run-kde` runs the same checks
# with a password that the demo guest takes from the host's directory.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import rdp as kde_rdp

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_rdp.run, machine, checks, SCALE))
checks.finish()
