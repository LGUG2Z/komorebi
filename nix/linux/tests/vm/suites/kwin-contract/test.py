# The contract of KWin's per-output virtual desktops that komorebi's workspaces rely on, checked
# against a headless KWin with two outputs and no komorebi: workspace `i` is desktop `i`, and each
# output shows a desktop of its own. What has to hold, and what breaks if it does not:
#
# - with `[Windows] PerOutputVirtualDesktops=true` in kwinrc, a script can show a desktop on one
#   output and the other output keeps its own (otherwise a workspace switch switches every monitor);
# - KWin says so with `workspace.currentDesktopChanged`, which the bridge listens to;
# - KWin focuses a window of the desktop it switched an output to, by itself (komorebi asks for no
#   focus after a switch it did not make);
# - activating a window on another desktop of its output (an app raising itself) makes KWin switch
#   that output to the window's desktop (which komorebi then follows);
# - a window moved by a script so that its middle is on the other output keeps its desktops: KWin
#   moves a window to the new output's desktop only during an interactive move;
# - a window put on another desktop from outside raises `desktopsChanged` on the window, which the
#   bridge listens to.
#
# docs/dev/PLATFORM.md records these as the facts of KWin that komorebi depends on.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import kwin_contract as kde_kwin_contract

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()

run_suite(checks, platform_only("kde", kde_kwin_contract.run, machine, checks, SCALE))
checks.finish()
