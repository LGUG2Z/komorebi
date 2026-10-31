# `initial_window_placement_rules` in a real (headless) KWin: where in the layout a window that
# opens goes. The daemon reads the rules from a `komorebi.json` it is started with. The shared
# scenario (`komorebi_vm.core.scenarios.placement`, which runs on Windows too) checks the places, by
# number and by title. What only a real KWin can tell is in
# `komorebi_vm.platforms.kde.extras.placement`: that the program a rule matches is the one a real
# window has (`foot.exe` being `foot`). The class (the Wayland app id) is a case of the shared one.
# `komorebi/tests/placement.rs` checks the same against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import placement
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import placement as kde_placement

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(1920, 1080), scale=SCALE)
platform = KdePlatform(kde, (1920, 1080))
platform.start_komorebi()

args = (platform, checks, [])
run_suite(
    checks,
    shared(placement.run, platform, checks),
    platform_only("kde", kde_placement.by_program, *args),
)

kde.stop_komorebi()
checks.check("a clean stop", not kde.komorebi_running())
kde.stop()
checks.finish()
