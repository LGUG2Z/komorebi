r"""KRDP, Plasma's RDP server, runs in the Plasma session of the session node as the guest to develop
in runs it (nix/dev/krdp.nix), and a client logs in and gets a picture of the desktop. What only a
real session can tell: that the unit starts in the session once it has a password, that KWin lets
KRDP have the screen without asking, that a client with the password is let in and one without
it is not, and that the picture the client gets is what the display shows.

The client is FreeRDP's, on an X server of its own (`komorebi_vm.RdpClient`), so its picture can be
read back and compared with a screenshot of the display. The suite `run-kde` runs the same checks
with a password that the demo guest takes from the host's directory.
"""

import secrets
import time
from typing import Any

from komorebi_vm import Checks, Kde, RdpClient
from komorebi_vm.rdp import colors

PASSWORD_FILE = "/home/komorebi/.config/komorebi-vm/rdp-password"


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `rdp`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.session(machine, scale=SCALE)
    rdp = RdpClient(machine, kde)

    def active(unit: str) -> bool:
        return kde.run(f"systemctl --user is-active --quiet {unit}")[0] == 0

    checks.check(
        "komorebi runs in the session", kde.wait_for(lambda: active("komorebi"), timeout=60)
    )
    checks.check("with no password, KRDP does not start", not active("krdp"))

    password = secrets.token_hex(16)
    kde.as_user(
        f"umask 077 && mkdir -p $(dirname {PASSWORD_FILE}) && printf %s {password} > {PASSWORD_FILE}"
    )
    kde.systemctl("start", "krdp.service")

    checks.check(
        "with a password, KRDP listens on port 3389", kde.wait_for(rdp.listening, timeout=60)
    )

    rdp.start_x_server()

    checks.check("a client with the password is let in", rdp.logs_in(password))
    checks.check("a client with another password is not", not rdp.logs_in("not-" + password))

    started = time.monotonic()
    rdp.connect(password)
    checks.check("the client's picture is the display", rdp.shows_display())
    print(f"after {time.monotonic() - started:.1f}s")

    remote = rdp.picture()
    print(f"the client's picture: {remote.size}, {colors(remote)} colors")
    remote.save(machine.out_dir / "client.png")
    print(machine.execute("journalctl --no-pager _SYSTEMD_USER_UNIT=krdp.service | tail -n 20")[1])
    print(machine.execute("tail -n 20 /tmp/xfreerdp.log")[1])
