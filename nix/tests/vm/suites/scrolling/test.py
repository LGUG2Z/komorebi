# The scrolling layout in a real KWin. The strip runs past the edge of the work area, and KWin is
# expected to leave the windows exactly where komorebi puts them, off the screen too. What only a
# real KWin can tell is in `komorebi_vm.platforms.kde.extras.scrolling`: that KWin keeps them there
# and that the strip scrolls with the focus `komorebic focus` moves. The shared scenario
# (`komorebi_vm.core.scenarios.scrolling`), which `core-e2e` runs and which runs on Windows too,
# checks the focused window is in view. `komorebi/tests/layout_options.rs` checks where komorebi
# puts the columns against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import scrolling as kde_scrolling

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)
platform = KdePlatform(kde, (W, H))
platform.start_komorebi()

run_suite(checks, platform_only("kde", kde_scrolling.run, platform, checks, []))

kde.stop()
checks.finish()
