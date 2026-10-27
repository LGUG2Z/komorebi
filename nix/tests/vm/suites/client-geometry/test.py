# How real clients answer the frame komorebi asks for. KWin carries out a request at once; the
# client then decides the size it draws at, and that is the frame KWin reports. What only real
# clients can tell, and what the fake desktop of the Rust tests cannot know: Qt and KDE programs
# and X11 programs on XWayland take the frame exactly, also below the minimum size they report;
# foot sizes itself down to whole cells of its font; GTK 4 programs and Firefox never go below
# their minimum size. Every client keeps the position komorebi asked for. The output is large
# enough that some tiles are bigger and some smaller than the clients' minimums.
from komorebi_vm import Checks, Kde, Rect, daemon

W, H = 3840, 2160
CELL = 24

EXACT = ["org.kde.konsole", "org.kde.kate", "org.kde.dolphin", "XTerm"]
WHOLE_CELLS = ["foot"]
NOT_BELOW_MINIMUM = ["org.gnome.TextEditor", "org.gnome.Calculator", "firefox"]

CLIENTS = {
    "foot": "foot -T foot-term sleep 3000",
    "org.kde.konsole": "konsole --separate",
    "org.kde.dolphin": "dolphin --new-window /tmp",
    "org.kde.kate": "kate --new",
    "org.gnome.TextEditor": "gnome-text-editor --new-window",
    "org.gnome.Calculator": "gnome-calculator",
    "XTerm": "xterm -T xterm-x11",
    "firefox": "firefox --no-remote --profile /tmp/firefox-profile about:blank",
}

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), xwayland=True, scale=SCALE)
kde.as_user("mkdir -p /tmp/firefox-profile")
kde.start_komorebi("--log-level", "debug")
checks.check(
    "the daemon loaded its script into KWin",
    kde.wait_for(lambda: "loaded the KWin bridge script" in kde.komorebi_log(), timeout=30),
)

for app, command in CLIENTS.items():
    kde.spawn(command, log=f"/tmp/{app}.log")
    up = kde.wait_for(lambda app=app: kde.wid_by_class(app) != "", timeout=120)
    if not checks.check(f"{app} has a window", up):
        print(machine.execute(f"cat /tmp/{app}.log")[1])


def shown() -> int:
    return sum(1 for e in daemon.events(kde.komorebi_log()) if e.kind == "Show")


checks.check("the daemon tiles all eight", kde.wait_for(lambda: shown() >= 8, timeout=60))


def show(r: Rect | None) -> str:
    return "-" if r is None else f"{r.x},{r.y} {r.w}x{r.h}"


def snapshot() -> dict[str, Rect]:
    return {app: kde.rect(kde.wid_by_class(app)) for app in CLIENTS}


# the clients answer in their own time; the frames are final once they stop changing
before: dict[str, Rect] = {}


def still() -> bool:
    global before
    now = snapshot()
    unchanged = now == before
    before = now
    return unchanged


kde.wait_for(still, timeout=60, interval=3)
requests = list(daemon.requests(kde.komorebi_log()).values())

print(f"     {'client':<22} {'asked':<22} {'has':<22}")
for app, have in snapshot().items():
    asked = [r for r in requests if (r.x, r.y) == (have.x, have.y)]
    want = asked[0] if asked else None
    print(f"     {app:<22} {show(want):<22} {show(have):<22}")
    if not checks.check(f"{app} is where komorebi put it", want is not None):
        continue
    assert want is not None
    if app in EXACT:
        checks.check(f"{app} takes the size it is asked for", (have.w, have.h) == (want.w, want.h))
    elif app in WHOLE_CELLS:
        checks.check(
            f"{app} sizes itself down to whole cells",
            0 <= want.w - have.w < CELL and 0 <= want.h - have.h < CELL,
        )
    elif app in NOT_BELOW_MINIMUM:
        checks.check(
            f"{app} is never smaller than it is asked to be",
            have.w >= want.w and have.h >= want.h,
        )

kde.stop_komorebi()
kde.stop()
checks.finish()
