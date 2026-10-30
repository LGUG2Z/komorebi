# What komorebi on Windows does itself and komorebi on KDE has the compositor do, in a real Plasma
# session whose machine has the effects installed (the module does that): what is on the screen,
# read from screenshots, not what komorebi wrote. Each feature is a section, and a run of the
# daemon:
#
#   A  toggle-maximize: the window fills the work area, is the one on top, and goes back to its
#      tile. It runs on the daemon the session started with the module's unit, as on a user's
#      machine; the sections after it stop that daemon and start their own runs
#   B  transparency: unfocused windows are see-through, the focused one is not
#   C  border colors: every kind of window has its own: the focused window, the others, a locked
#      container that does not have the focus, a stack and a monocled window that have it, and a
#      floating window that has it
#   D  animation curves: two curves put a window in different places at the same moment
#   E  fullscreen: a fullscreen window covers the monitor, is not moved, and goes back to its tile
#
# Every window is a `foot` of one color all over (`Kde.colored_window`), and a color is read at the
# pixels just outside the frame of the window (`pixels.ring`); section E reads the frames KWin
# reports for the windows, not a screenshot. `komorebi/tests/maximize.rs`, `maximize_commands.rs`
# and `fullscreen.rs` check what the window manager does with a maximize and a fullscreen against
# the fake; section A checks that KWin's own maximize does it, and section E that a window KWin has
# taken fullscreen is left alone.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import features as kde_features

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_features.run, machine, checks, SCALE))
checks.finish()
