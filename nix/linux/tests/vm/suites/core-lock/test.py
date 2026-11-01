# The neutral lock scenario, on a real Plasma session with komorebi running as the module's user
# unit: the screen is locked and unlocked through logind, and what the scenario looks at is the
# desktop, not the daemon's log. The same scenario runs against a Windows guest (`LockWorkStation`).
# `lock-sleep` keeps what is only KDE's: the daemon's log of the lock screen, and the signals.
from komorebi_vm import Checks, Kde
from komorebi_vm.core.scenarios import locking
from komorebi_vm.core.suite import run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.recorder import Recorder

TITLES = ["lock-a", "lock-b", "lock-c"]

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)
platform = KdePlatform(kde, (1920, 1080), session=True)

checks.check(
    "the module's unit runs the daemon, which answers komorebic",
    kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=60),
)
# the daemon tiles again when it learns of the panel's room, so the windows open after that
kde.wait_for_panel_room()
recorder = Recorder(kde, RECORDER)
recorder.start("core-lock")
clients = [platform.spawn(title) for title in TITLES]
checks.check(
    "the daemon manages the three windows",
    platform.wait_for(lambda: platform.state().managed() == len(clients), timeout=30),
)

run_suite(checks, shared(locking.run, platform, checks, clients, always=True))

recorder.stop()
checks.finish()
