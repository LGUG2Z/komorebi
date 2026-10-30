"""What only KDE can tell about the commands, round the shared `commands` scenarios.

Every window the daemon has is a window KWin has, and the frame the daemon last asked KWin for is
the one KWin carries out. The shared scenarios check the layouts with five windows and that every
command is understood and leaves the daemon healthy, as the operating system sees it; what is here
reads KWin (`kdotool`) and the daemon's log.

The suite runs `setup`, `layouts_by_count`, the shared `layouts` and `sweep`,
`replace_configuration`, `handover`, the shared `title_rule` and `prints`, `queries` and
`at_the_end`.
The daemon runs on a session with two outputs, and `clients` are five windows on the first.
"""

from komorebi_vm import daemon
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import CELL, KdePlatform

OUTPUTS = 2
LAYOUTS = (
    "bsp",
    "columns",
    "rows",
    "vertical-stack",
    "horizontal-stack",
    "ultrawide-vertical-stack",
    "grid",
    "right-main-vertical-stack",
    "scrolling",
)
QUERIES = (
    "focused-monitor-index",
    "focused-workspace-index",
    "focused-container-index",
    "focused-window-index",
    "focused-workspace-name",
    "focused-workspace-layout",
    "focused-container-kind",
    "version",
)


def _live_windows(platform: KdePlatform) -> list[str]:
    """The ids of the windows KWin has: every client of the suite is a foot."""
    return platform.kde.run("kdotool search --class foot")[1].split()


def healthy(platform: KdePlatform) -> int:
    """0 when the daemon is running, answers `komorebic state` and has every window KWin has.

    Otherwise the reason: 1 not running, 2 no state, 3 not the windows KWin has.
    """
    kde = platform.kde
    if not kde.komorebi_running():
        return 1
    if kde.run("komorebic state")[0] != 0:
        return 2
    if kde.managed_all() != len(_live_windows(platform)):
        return 3
    return 0


def _outside_or_overlapping(platform: KdePlatform) -> list[str]:
    """The windows that are outside the outputs, or on another window (2 pixels are allowed)."""
    kde = platform.kde
    width, height = platform.size
    rects = {wid: kde.rect(wid) for wid in _live_windows(platform)}
    bad = []
    for wid, r in rects.items():
        if r.x < -1 or r.y < -1 or r.x + r.w > width + 1 or r.y + r.h > height + 1:
            bad.append(f"{kde.title(wid)} outside at {r}")
    ids = list(rects)
    for i, a in enumerate(ids):
        for b in ids[i + 1 :]:
            ra, rb = rects[a], rects[b]
            if (
                ra.x < rb.x + rb.w - 2
                and rb.x < ra.x + ra.w - 2
                and ra.y < rb.y + rb.h - 2
                and rb.y < ra.y + ra.h - 2
            ):
                bad.append(f"{kde.title(a)} on {kde.title(b)}")
    return bad


def _open_window(platform: KdePlatform, n: int) -> None:
    platform.spawn(f"win{n}")


def _close_window(platform: KdePlatform, n: int) -> None:
    kde = platform.kde
    title = f"win{n}"
    kde.close(kde.wid(title))
    kde.wait_for(lambda: kde.wid(title) == "", timeout=10)


def _where_asked(platform: KdePlatform, wid: str) -> str | None:
    """None when KWin has the window where the daemon last asked, else what is wrong."""
    kde = platform.kde
    title = kde.title(wid)
    want, have = daemon.asked(kde.komorebi_log(), title), kde.rect(wid)
    if want is None:
        return f"{title}: the daemon never asked for a frame"
    if (have.x, have.y) != (want.x, want.y) or not (
        0 <= want.w - have.w < CELL and 0 <= want.h - have.h < CELL
    ):
        return f"{title}: asked {want}, has {have}"
    return None


def setup(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The daemon answers with both outputs and has the five windows, as KWin has them."""
    kde = platform.kde
    checks.check(
        "the daemon answers komorebic, with both outputs",
        kde.wait_for(
            lambda: (
                kde.run("komorebic monitor-information")[0] == 0
                and len(kde.state()["monitors"]["elements"]) == OUTPUTS
            ),
            timeout=30,
        ),
    )
    checks.check(
        "five windows, and the daemon has all five",
        kde.wait_for(lambda: kde.managed() == len(clients) and healthy(platform) == 0, timeout=20),
    )


def layouts_by_count(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Every layout with five, three, two and one windows: the windows are in their outputs and
    apart, the daemon is healthy, and KWin has each window where the daemon last asked for it.

    The shared `layouts` checks five windows. The windows that were closed are opened again, and
    the layout is `bsp` at the end.
    """
    kde = platform.kde
    for layout in LAYOUTS:
        kde.komorebic("change-layout", layout)
        verdict = []
        for keep in (5, 3, 2, 1):
            while kde.managed() > keep:
                _close_window(platform, kde.managed())
                kde.wait_for(lambda: healthy(platform) == 0, timeout=10)
            # the daemon retiles on the close; give KWin the time to carry it out
            kde.wait_for(
                lambda: all(_where_asked(platform, w) is None for w in _live_windows(platform)),
                timeout=10,
            )
            # the scrolling layout is a strip whose columns can be past the edge of the output
            if layout != "scrolling":
                verdict += [f"{keep}w: {bad}" for bad in _outside_or_overlapping(platform)]
            if reason := healthy(platform):
                verdict.append(f"{keep}w: not healthy ({reason})")
        checks.check(
            f"{layout}: the windows are in their outputs and do not overlap, and the daemon is"
            " healthy" + "".join(f"\n       {v}" for v in verdict),
            verdict == [],
        )
        for n in range(kde.managed() + 1, len(clients) + 1):
            _open_window(platform, n)
        kde.wait_for(lambda: kde.managed() == len(clients), timeout=20)
    kde.komorebic("change-layout", "bsp")


def replace_configuration(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """After the shared sweep ran `replace-configuration`, the window manager is made again from
    the file, whose first workspace is named `again`."""
    first = platform.kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
    checks.check(
        "replace-configuration made the window manager again from the file",
        first["name"] == "again",
    )


def handover(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """`minimize`, `unmanage` and `manage` on the focused window: the daemon lets go of it, and
    takes it up again.

    They take a window out of the daemon's hands, so the shared sweep does not run them.
    """
    kde = platform.kde
    focused = kde.active()
    title = kde.title(focused)
    before = kde.managed()
    kde.wait_for(lambda: kde.focused_title() == title, timeout=10)
    kde.komorebic("minimize")
    checks.check(
        f"minimize: the daemon lets go of {title} when KWin minimizes it",
        kde.wait_for(lambda: kde.managed() == before - 1, timeout=10) and kde.wid(title) != "",
    )
    kde.activate(focused)
    checks.check(
        f"{title}, restored, is tiled again",
        kde.wait_for(lambda: kde.managed() == before and healthy(platform) == 0, timeout=10),
    )
    kde.wait_for(lambda: kde.focused_title() == title, timeout=10)
    kde.komorebic("unmanage")
    checks.check(
        f"unmanage: the daemon lets go of {title}, which stays open",
        kde.wait_for(lambda: kde.managed() == before - 1, timeout=10) and kde.wid(title) != "",
    )
    width, height = platform.size[0] // OUTPUTS, platform.size[1]

    def centered() -> bool:
        r = kde.rect(focused)
        return (
            abs(r.x + r.w // 2 - width // 2) <= CELL and abs(r.y + r.h // 2 - height // 2) <= CELL
        )

    checks.check(f"{title} is in the middle of the work area", kde.wait_for(centered, timeout=10))
    kde.komorebic("manage")
    checks.check(
        f"manage: {title} is tiled again",
        kde.wait_for(lambda: kde.managed() == before and healthy(platform) == 0, timeout=10),
    )


def queries(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """`komorebic query` answers, and the resize commands that save and load are understood."""
    kde = platform.kde
    for query in QUERIES:
        status, out = kde.run(f"komorebic query {query}")
        checks.check(f"query {query} answers", status == 0 and out.strip() != "")
    checks.check(
        "quick-save-resize and quick-load-resize",
        kde.run("komorebic quick-save-resize && komorebic quick-load-resize")[0] == 0,
    )
    checks.check(
        "save-resize and load-resize",
        kde.run(
            "komorebic save-resize /tmp/e2e-resize.json"
            " && komorebic load-resize /tmp/e2e-resize.json"
        )[0]
        == 0,
    )


def at_the_end(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The daemon has every window KWin has, each is where the daemon last asked for it, and a
    clean stop leaves the daemon not running."""
    kde = platform.kde
    checks.check("the daemon has every window KWin has, and answers", healthy(platform) == 0)
    for wid in _live_windows(platform):
        print(f"     {kde.title(wid)}: {kde.rect(wid)}")
    mismatch = [m for m in (_where_asked(platform, w) for w in _live_windows(platform)) if m]
    for m in mismatch:
        print(f"     {m}")
    checks.check("every window is where the daemon last asked for it", mismatch == [])
    kde.komorebic("stop")
    checks.check("a clean stop", kde.wait_for(lambda: not kde.komorebi_running()))
