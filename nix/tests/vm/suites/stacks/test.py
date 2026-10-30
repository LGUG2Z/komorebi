# Stacking on a real KWin: what komorebi believes (`komorebic state`) against what KWin actually
# has (`kdotool`). The checks are `komorebi_vm.platforms.kde.extras.stacks`: the parking desktop is
# made when it is needed, removed when komorebi stops, and swept after a crash; the windows a stack
# hides are on a desktop of their own; the window in view is the one both sides have focused. The
# shared `stacks` and `monocle` scenarios, which `core-e2e` runs, check the same as the operating
# system sees it. `komorebi/tests/stacks.rs` checks which windows a stack holds against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import stacks as kde_stacks

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)
platform = KdePlatform(kde, (W, H))

run_suite(checks, platform_only("kde", kde_stacks.run, platform, checks, []))

kde.stop()
checks.finish()
