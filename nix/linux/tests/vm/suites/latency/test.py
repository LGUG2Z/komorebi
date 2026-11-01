# How long a retile takes to reach the windows of a real KWin, measured on the session bus: from
# the moment the daemon answers the bridge's held call with a batch of new frames, to the last
# frame KWin reports for one of the windows it moved, the client's answer included. Every number
# here comes from KWin and real clients; the Rust tests cannot measure it. The output is large, so
# that ten windows in komorebi's default layout all have room.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import latency as kde_latency

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_latency.run, machine, checks, SCALE))
checks.finish()
