# Wallpapers, and the borders' colors that come from them, in a real Plasma session: what the shell
# shows and what is on the screen, read from the shell's D-Bus API and from screenshots.
#
#   A  a monitor's wallpaper is set at start, and a workspace's own replaces it when it is shown
#   B  a switch komorebi did not make (a pager's) sets the wallpaper too
#   C  the borders take the colors of the wallpaper's palette, and follow it from workspace to
#      workspace
#   D  a wallpaper the user changed by hand is put back when its workspace is shown again
#   E  the palette cache, and a path with a space in it (the second picture's)
#   F  komorebi that starts before the Plasma shell does
#
# The pictures are a dozen bands of color each, since a palette wants at least eight: a warm one for
# the monitor, a cool one for the second workspace, and a green one the test sets by hand.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import wallpaper as kde_wallpaper

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_wallpaper.run, machine, checks, SCALE))
checks.finish()
