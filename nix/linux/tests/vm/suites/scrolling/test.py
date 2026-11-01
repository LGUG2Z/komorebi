# The scrolling layout in a real KWin, with the shared scenario `scrolling.columns`
# (`komorebi_vm.core.scenarios.scrolling`), which runs on Windows too: the strip runs past the edge
# of the work area, a window that is out of view rests at the edge it left by with a few pixels of
# it on the screen, a strip with a monitor next to it leaks no window onto it (`two_monitors`), the strip scrolls with the focus and with `pan`, and the commands of the columns
# (`consume`, `expel`, `toggle-column-stack`, `scrolling-layout-full-width`) work. `core-e2e` runs it
# too, among the scenarios that start from a workspace with no window.
# `komorebi/tests/layout_options.rs` checks where komorebi puts the columns against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import scrolling
from komorebi_vm.core.suite import run_suite, shared
from komorebi_vm.kde_platform import KdePlatform

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
# two outputs, of which the second is unplugged before the daemon starts: `two_monitors` plugs it in
kde = Kde.headless(machine, "main", size=(W, H), outputs=2, scale=SCALE)
platform = KdePlatform(kde, (W, H))
platform.remove_display()
platform.start_komorebi()

run_suite(
    checks,
    shared(scrolling.columns, platform, checks, []),
    shared(scrolling.two_monitors, platform, checks, []),
)

kde.stop()
checks.finish()
