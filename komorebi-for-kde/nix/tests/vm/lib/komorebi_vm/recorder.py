"""`komorebi-recorder` alongside a suite: what the daemon and the desktop did, as an artifact.

The recorder is optional. Its repository is private, so it is not an input of the flake: a run that
wants it builds it elsewhere and passes the store path as `KOMOREBI_RECORDER`, evaluating with
`--impure` (see docs/dev/vm-tests.md). A run without it records nothing and says so.

The recording has no video: a headless KWin has no screen to capture, and the log, the daemon's
events and the probes of the windows are what a failed check is read against.
"""

import shlex

from komorebi_vm.kde import Kde

PROGRAM = "komorebi-recorder"


class Recorder:
    """A recording of the daemon of `kde`, taken in the guest as the test user."""

    def __init__(self, kde: Kde, enabled: bool) -> None:
        self.kde = kde
        self.enabled = enabled
        self.out = ""
        self.pid = ""

    def start(self, name: str) -> None:
        """Starts recording the session into a directory named `name`, and waits until the
        recorder has made it."""
        if not self.enabled:
            print("   (no recorder in this run: set KOMOREBI_RECORDER to record)")
            return
        self.out = f"/tmp/recording-{name}"
        log = f"{self.out}.log"
        command = f"{PROGRAM} record --no-video --probe-windows --out {shlex.quote(self.out)}"
        self.pid = self.kde.sh(
            f"setsid nohup {command} > {shlex.quote(log)} 2>&1 < /dev/null & echo $!"
        ).strip()
        self.kde.wait_for(lambda: self.kde.run(f"test -d {shlex.quote(self.out)}")[0] == 0)

    def stop(self) -> None:
        """Stops the recording, prints what the recorder makes of it, and keeps the session as
        `recording-NAME.tar.gz` with the artifacts of the run."""
        if not self.enabled or not self.pid:
            return
        self.kde.run(f"kill -INT {self.pid}")
        self.kde.wait_for(lambda: self.kde.run(f"kill -0 {self.pid}")[0] != 0, timeout=60)
        out = shlex.quote(self.out)
        print(self.kde.run(f"{PROGRAM} brief {out} --compact")[1])
        print(self.kde.run(f"{PROGRAM} findings {out}")[1])
        archive = f"{self.out}.tar.gz"
        self.kde.run(f"tar czf {shlex.quote(archive)} -C /tmp {shlex.quote(self.out[5:])}")
        self.kde.machine.copy_from_machine(archive)
        self.pid = ""
