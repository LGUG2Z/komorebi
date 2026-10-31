# The border and the animation, in a real Plasma session whose machine has the effects installed
# (the module does that): komorebi writes the effects' settings, loads them, and on exit puts
# everything back.
#
# What is checked is what is on the screen, not what komorebi wrote: a screenshot is read at the
# pixels just outside each window's frame (the border) and at the place a window is going to (the
# animation). The runs of komorebi:
#
#   A  the borders, as they are by default: the ring is outside the frame on all four sides
#   B  the commands that change them: the effect follows, and takes the border away and back
#   C  a 30 s animation: a window flipped to the other side is not there yet moments later
#   D  no animation, the control: the same flip is there at once
#
# Every window is a `foot` of one color all over (`Kde.colored_window`).

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import look as kde_look

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_look.run, machine, checks, SCALE))
checks.finish()
