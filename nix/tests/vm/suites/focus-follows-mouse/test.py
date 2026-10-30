# Focus follows mouse against a real KWin session, with a real pointer.
#
# komorebi's focus follows mouse is KWin's own focus policy: the window under the pointer is the one
# that has the focus, and komorebi follows. Checked by moving the pointer over each window in turn,
# then turning it off (the focus stays where it was), and on exit (the user's policy is back in
# `kwinrc`). The pointer is a device made inside the guest (`Pointer`, the program
# `komorebi-pointer`), moved to the middle of a window by pixel.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import focus_follows_mouse as kde_focus_follows_mouse

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_focus_follows_mouse.run, machine, checks, SCALE))
checks.finish()
