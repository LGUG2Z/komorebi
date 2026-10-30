# The tiles next to an edge follow it while it is dragged, not only when it is let go, in a Plasma
# session with a pointer device the test drives from inside the guest (`komorebi_vm.Pointer`). For
# each case the button is pressed on an edge of a window, the pointer is taken part of the way and
# held there, and the frames of every window are read while the button is still down; then the
# rest of the way, read again, and let go (`komorebi/tests/live_resize.rs` checks the window
# manager's side against the fake). What only a real KWin can tell is that the bridge sends the
# steps of a drag and that the neighbors are moved while KWin has the dragged window:
#
# * part way and all the way, the neighbor's facing edge has moved with the pointer, give or take
#   a cell of the terminal's font
# * the windows that are not next to the dragged edge have not moved
# * when the button is let go nothing jumps: the neighbors stay where they followed to
# * the windows still tile

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import live_resize as kde_live_resize

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_live_resize.run, machine, checks, SCALE))
checks.finish()
