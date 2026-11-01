# Every layout, and a sweep of komorebic's commands, against a real headless KWin with two outputs.
#
# After each command the same things have to be true: the daemon is alive, it can still say what it
# knows (`komorebic state`), and every window it has is a window the system has. The shared scenarios
# (`komorebi_vm.core.scenarios.commands`, which run on Windows too) check that, for every layout with
# five windows and for every command, and that a command is understood (exit status 0). What only
# a real KWin can tell is in `komorebi_vm.platforms.kde.extras.commands`: every layout with fewer
# windows, and that what the daemon asks KWin for is what KWin carries out. The commands that take
# a window out of the daemon's hands and a title rule are shared scenarios. The Rust tests check
# what each command does against the fake.
import shlex

from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import commands, handover, queries, title_rule
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import commands as kde_commands

W, H = 1920, 1080
OUTPUTS = 2
CONFIG = "/tmp/e2e-config.json"

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), outputs=OUTPUTS, scale=SCALE)
platform = KdePlatform(kde, (W * OUTPUTS, H))

platform.start_komorebi()
clients = [platform.spawn(f"win{n}") for n in range(1, 6)]
kde.sh(
    "printf %s "
    + shlex.quote('{ "monitors": [ { "workspaces": [ { "name": "again", "layout": "BSP" } ] } ] }')
    + f" > {CONFIG}"
)

args = (platform, checks, clients)
run_suite(
    checks,
    platform_only("kde", kde_commands.setup, *args),
    platform_only("kde", kde_commands.layouts_by_count, *args),
    shared(commands.layouts, *args),
    platform_only("kde", kde.komorebic, "change-layout", "bsp"),
    shared(commands.sweep, *args, CONFIG),
    platform_only("kde", kde_commands.replace_configuration, *args),
    platform_only("kde", kde_commands.bring_back, *args),
    shared(handover.run, *args),
    shared(title_rule.run, platform, checks),
    shared(commands.prints, platform, checks),
    shared(queries.run, platform, checks),
    platform_only("kde", kde_commands.at_the_end, *args),
)

kde.stop()
checks.finish()
