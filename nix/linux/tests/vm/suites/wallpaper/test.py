# Wallpapers, and the borders' colors that come from them, in a real Plasma session: what the shell
# shows and what is on the screen, read from the shell's D-Bus API and from screenshots.
#
# The shared scenario `wallpaper` (`komorebi_vm.core.scenarios.wallpaper`, which runs on Windows
# too) sets a wallpaper for a monitor and one for a workspace, follows them from workspace to
# workspace with the colors of the borders, and sets one back that the user changed. The extra
# is what only the Plasma shell tells:
#
#   A  the fill mode of the wallpaper, and how long a workspace takes to change it
#   B  a switch komorebi did not make (a pager's) sets the wallpaper too
#   E  the palette cache
#   F  komorebi that starts before the Plasma shell does

from komorebi_vm import Checks
from komorebi_vm import Kde
from komorebi_vm.core.scenarios import wallpaper
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import wallpaper as kde_wallpaper

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)
# the session started the module's unit when it logged in: the scenario restarts that daemon with
# the wallpapers it writes to the user's `komorebi.json`
checks.check("komorebi answers", kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, 120))
kde.wait_for_panel_room()
size = kde.state()["monitors"]["elements"][0]["size"]
platform = KdePlatform(kde, (size["right"], size["bottom"]), session=True)

run_suite(
    checks,
    shared(wallpaper.run, platform, checks),
    platform_only("kde", kde_wallpaper.run, machine, checks, SCALE),
)
checks.finish()
