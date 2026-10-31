# komorebi's workspaces are KWin's own virtual desktops, per output. What only a real KWin can
# tell is in `komorebi_vm.platforms.kde.extras.workspace_switch`: that komorebi turns on KWin's
# per-output virtual desktops and puts the user's setting back when it stops, that a window sent to
# another workspace is on another desktop while the one in front keeps showing, that
# `focus-workspace` has KWin show the desktop and focus the window there, that a switch by anything
# else (a pager, a shortcut, an app raising a window) is followed once without flip-flopping, and
# that a window moved to another desktop by someone else moves to that workspace. The shared
# `workspaces` scenario, which `core-e2e` runs, checks the commands put windows away and back.
# `komorebi/tests/workspaces.rs` checks the same against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import workspace_switch as kde_switch

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(1920, 1080), scale=SCALE)
platform = KdePlatform(kde, (1920, 1080))

run_suite(checks, platform_only("kde", kde_switch.run, platform, checks, []))

kde.stop()
checks.finish()
