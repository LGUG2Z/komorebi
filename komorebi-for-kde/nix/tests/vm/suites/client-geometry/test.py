# How real clients answer the frame komorebi asks for. KWin carries out a request at once; the
# client then decides the size it draws at, and that is the frame KWin reports. What only real
# clients can tell, and what the fake desktop of the Rust tests cannot know: Qt and KDE programs
# and X11 programs on XWayland take the frame exactly, also below the minimum size they report;
# foot sizes itself down to whole cells of its font; GTK 4 programs and Firefox never go below
# their minimum size. Every client keeps the position komorebi asked for. The output is large
# enough that some tiles are bigger and some smaller than the clients' minimums.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import client_geometry as kde_client_geometry

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_client_geometry.run, machine, checks, SCALE))
checks.finish()
