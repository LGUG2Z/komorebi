r"""What the harness itself promises: a headless KWin comes up with the outputs asked for, a client
gets a window on it, and the library sees both.
"""

from typing import Any

from komorebi_vm import Checks, Kde


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `kwin`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.headless(machine, "main", size=(1280, 720), outputs=2, scale=SCALE)
    checks.check("KWin owns its name on the private bus", kde.has_name("org.kde.KWin"))
    checks.check("KWin has the two outputs that were asked for", lambda: kde.outputs() == 2)

    kde.spawn("foot -T alpha sleep 3000")
    checks.check("a window appears", kde.wait_for(lambda: kde.wid("alpha") != "", timeout=30))
    wid = kde.wid("alpha")
    checks.check("the library reads its title back", lambda: kde.title(wid) == "alpha")
    rect = kde.rect(wid)
    print(f"alpha is at {rect}")
    checks.check("it has a size", rect.w > 0 and rect.h > 0)

    other = Kde.headless(machine, "other", size=(800, 600), scale=SCALE)
    checks.check("a second session is its own KWin", lambda: other.outputs() == 1)
    checks.check("it does not see the first session's window", other.wid("alpha") == "")
    other.stop()

    kde.stop()
    checks.check("stopping the session releases its name", not kde.has_name("org.kde.KWin"))
