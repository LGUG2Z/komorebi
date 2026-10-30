# Workspace rules in a real KWin: a rule sends the windows of an application to a workspace, which
# is a virtual desktop, so that a window that opens on the desktop in front leaves the workspace in
# front and is hidden, on the parking desktop, until its workspace is shown. The shared scenarios
# (`komorebi_vm.core.scenarios.rules` and `startup_rules`, which run on Windows too) check where the
# daemon puts the window, that an initial rule sends it once and a permanent rule sends it again,
# and the rules of a `komorebi.json` for a window that was open before the daemon. What only a real
# KWin can tell is in `komorebi_vm.platforms.kde.extras.workspace_rules`: that the window is on the
# desktop of its workspace and out of sight on the parking desktop while another desktop shows,
# the rules by the name of a workspace, and a rule applied at the next event.
# `komorebi/tests/workspace_rules.rs` checks the same against the fake.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import rules, startup_rules
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import workspace_rules as kde_rules

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
    platform_only("kde", kde_rules.desktops, *args),
    platform_only("kde", kde_rules.named_rule, *args),
    platform_only("kde", kde_rules.cleared, *args),
    shared(rules.run, platform, checks),
    shared(startup_rules.run, platform, checks),
)

kde.stop_komorebi()
checks.check("a clean stop", not kde.komorebi_running())
kde.stop()
checks.finish()
