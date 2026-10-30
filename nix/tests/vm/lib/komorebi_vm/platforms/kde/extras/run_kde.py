r"""The demo guest of `run-kde` (nix/dev/run-kde.nix), as the command starts it: the module is the
session node's. What only a real session can tell: that the demo opens its windows and komorebi
tiles them clear of the panel, that the cheat sheet is a window komorebi leaves alone, that what
the cheat sheet shows of `komorebic check` has no error in it, and that the password and the key
the host shares reach the units that use them. The host's directory is made as the guest finds it
when it has mounted it (the node says how): another user owns it, so that the guest's own user
cannot read what is in it, and the guest has to take it as root.

The RDP checks are those of the suite `rdp`, with the password from that directory. Nothing starts
KRDP by hand here: that it listens shows that the guest took the password before the login, as the
command's guest has to. The command itself, `scripts/run-kde`, is checked by `run-kde-dry-run` and
`run-kde-doctor`, which a VM cannot: it is a program of the host.
"""

import re
from typing import Any

from komorebi_vm import Checks, Kde, RdpClient
from komorebi_vm.rdp import colors

SHARED = "/run/run-kde"
TAKEN = "/run/run-kde-guest"
PASSWORD = "a-password-of-the-host"
CHEAT_SHEET_TITLE = "komorebi cheat sheet"


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `run-kde`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.session(machine, scale=SCALE)
    rdp = RdpClient(machine, kde)

    def active(unit: str) -> bool:
        return kde.run(f"systemctl --user is-active --quiet {unit}")[0] == 0

    def tiles() -> list[str]:
        """The titles of the tiled windows of the first workspace, or none while the daemon is busy."""
        try:
            return [title for container in kde.containers(0) for title in container]
        except AssertionError:
            return []

    def demo_is_open() -> bool:
        titles = tiles()
        return (
            sum("Konsole" in title for title in titles) == 2
            and any("Kate" in title for title in titles)
            and any("Dolphin" in title for title in titles)
        )

    checks.check(
        "komorebi runs in the session", kde.wait_for(lambda: active("komorebi"), timeout=60)
    )

    print("== the windows of the demo")
    checks.check(
        "the demo opens two Konsole windows, Kate and Dolphin, and komorebi tiles them",
        kde.wait_for(demo_is_open, timeout=240),
    )
    checks.check(
        "the cheat sheet is a window",
        kde.wait_for(lambda: kde.wid(CHEAT_SHEET_TITLE) != "", timeout=120),
    )
    titles = tiles()
    print(f"the tiles: {titles}")
    checks.check("the cheat sheet is not one of the tiles", len(titles) == 4)

    # a window tiled before komorebi knew the panel's room is tiled under the panel
    monitor = kde.state()["monitors"]["elements"][0]
    work = monitor["work_area_size"]
    work_bottom = work["top"] + work["bottom"]
    bottoms = {title: (lambda r: r.y + r.h)(kde.rect(kde.wid(title))) for title in titles}
    print(f"the bottom of the work area: {work_bottom}; of the tiles: {bottoms}")
    checks.check(
        "every tile ends above the panel",
        all(bottom <= work_bottom for bottom in bottoms.values()),
    )
    checks.check("the screen shows a desktop", colors(kde.shot("demo")) > 1000)

    print("== the cheat sheet")
    checks.check(
        "the cheat sheet has the bindings of the sample file",
        machine.execute("grep -q 'alt + shift - 0x21' /etc/run-kde/cheat-sheet.txt")[0] == 0
        and machine.execute("grep -q 'komorebic change-layout' /etc/run-kde/cheat-sheet.txt")[0]
        == 0,
    )
    checks.check(
        "KWin's renderer is found, and said as the cheat sheet says it",
        kde.wait_for(
            lambda: machine.execute(f"test -s /run/user/{kde.uid}/run-kde-renderer")[0] == 0,
            timeout=60,
        ),
    )
    renderer = machine.succeed(f"cat /run/user/{kde.uid}/run-kde-renderer").strip()
    print(f"the renderer: {renderer}")
    checks.check(
        "a node with no GPU draws in software",
        re.match(r"software \(llvmpipe\): ", renderer) is not None,
    )
    checks.check(
        "the console is told too",
        machine.execute(
            "journalctl -b -u run-kde-report --no-pager | grep -q 'run-kde: KWin draws with'"
        )[0]
        == 0,
    )

    status, out = kde.run("KOMOREBI_CONFIG_HOME=/etc/run-kde/config-home komorebic check")
    print(out)
    checks.check(
        "komorebic check, as the cheat sheet runs it, has no error",
        status == 0 and "\n0 error(s)" in out,
    )
    checks.check(
        "it reads the key bindings the daemon runs",
        re.search(r"ok +\S*skhdrc has \d+ key bindings?", out) is not None,
    )
    checks.check(
        "it sees komorebi, and its script in KWin",
        "komorebi is running, and its script is in KWin" in out,
    )

    print("== what the host shares")
    checks.check(
        "the guest's user cannot read the host's directory",
        machine.execute(f"runuser -u komorebi -- cat {SHARED}/rdp-password")[0] != 0,
    )
    checks.check(
        "the password is taken for the user, and for no one else",
        machine.succeed(f"stat -c '%U %a' {TAKEN}/rdp-password").strip() == "komorebi 400",
    )
    checks.check(
        "the key is taken as the user's authorized key",
        machine.succeed(f"cat {TAKEN}/authorized_keys.d/komorebi").split()[:2]
        == machine.succeed(f"cut -d' ' -f1,2 {SHARED}/ssh_key.pub").split(),
    )
    machine.succeed("install -m 600 /etc/run-kde-test/ssh_key /tmp/ssh_key")
    ssh = (
        "ssh -i /tmp/ssh_key -o IdentitiesOnly=yes -o StrictHostKeyChecking=no"
        " -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o LogLevel=ERROR komorebi@127.0.0.1"
    )
    machine.wait_for_unit("sshd.service")
    checks.check("ssh lets in the host's key", machine.execute(f"{ssh} true")[0] == 0)
    checks.check(
        "and no other",
        machine.execute(
            "ssh-keygen -q -t ed25519 -N '' -f /tmp/other_key"
            f" && {ssh.replace('/tmp/ssh_key', '/tmp/other_key')} true"
        )[0]
        != 0,
    )

    print("== RDP, with the host's password")
    checks.check(
        "KRDP started by itself and listens on port 3389",
        kde.wait_for(lambda: active("krdp") and rdp.listening(), timeout=60),
    )
    rdp.start_x_server()
    checks.check("a client with the host's password is let in", rdp.logs_in(PASSWORD))
    checks.check("a client with another password is not", not rdp.logs_in("not-" + PASSWORD))
    rdp.connect(PASSWORD)
    checks.check("the client's picture is the display", rdp.shows_display())
    rdp.picture().save(machine.out_dir / "client.png")
