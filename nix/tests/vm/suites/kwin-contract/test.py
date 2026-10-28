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
from itertools import count

from komorebi_vm import Checks, Kde

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
# per-output desktops are read when KWin starts, as komorebi's users have them after their first run
machine.succeed(
    "runuser -u komorebi -- sh -c 'mkdir -p ~/.config"
    ' && printf "[Windows]\\nPerOutputVirtualDesktops=true\\n" > ~/.config/kwinrc\''
)
kde = Kde.headless(machine, "main", size=(1920, 1080), outputs=2, scale=SCALE)

# what every script is given: the desktop each output shows, and a window by its title
HELPERS = """
function cur() {
    return workspace.screens.map(function (o) {
        return workspace.currentDesktopForScreen(o).name;
    }).join("/");
}
function win(title) {
    return workspace.windowList().filter(function (w) { return w.caption === title; })[0];
}
function names(ds) {
    return ds.map(function (d) { return d.name; }).join(",");
}
"""


def script(name: str, body: str, keep: bool = False) -> list[str]:
    return kde.kwin_script(name, HELPERS + body + '\nreport("done");\n', keep=keep)


def shows() -> str:
    """What each output shows, as `first/second`."""
    lines = script(f"shows-{next(counter)}", "report(cur());")
    return lines[0] if lines else ""


counter = count(1)

print("== 1. two outputs, one desktop, and per-output desktops on")
outputs = script(
    "outputs", 'report(workspace.screens.map(function (o) { return o.name; }).join(" "));'
)
print(f"     outputs: {outputs}")
names_of = outputs[0].split() if outputs else []
checks.check("KWin has two outputs", len(names_of) == 2)
checks.check("and one desktop", kde.desktops() == 1)
checks.check(
    "kwinrc has per-output desktops on",
    kde.kwinrc("Windows", "PerOutputVirtualDesktops") == "true",
)

print("== 2. a desktop created over D-Bus is a desktop of every output")
kde.sh(
    "busctl --user call org.kde.KWin /VirtualDesktopManager"
    " org.kde.KWin.VirtualDesktopManager createDesktop us 1 two > /dev/null"
)
checks.check("KWin has two desktops", kde.wait_for(lambda: kde.desktops() == 2))
desktops = script("desktops", "report(names(workspace.desktops));")
checks.check(f"the script sees them in order: {desktops}", desktops[-1:] == ["Desktop 1,two"])
checks.check("both outputs show the first", shows() == "Desktop 1/Desktop 1")

print("== 3. one output shows another desktop, and the other keeps its own")
script(
    "listen",
    'workspace.currentDesktopChanged.connect(function () { report("changed", cur()); });',
    keep=True,
)
script(
    "switch", "workspace.setCurrentDesktopForScreen(workspace.desktops[1], workspace.screens[0]);"
)
checks.check(
    "the first output shows the second desktop, and the second output the first",
    kde.wait_for(lambda: shows() == "two/Desktop 1"),
)
checks.check(
    "KWin said so with currentDesktopChanged",
    kde.wait_for(lambda: "changed two/Desktop 1" in kde.kwin_reports("listen")),
)
script("back", "workspace.setCurrentDesktopForScreen(workspace.desktops[0], workspace.screens[0]);")
checks.check("and back", kde.wait_for(lambda: shows() == "Desktop 1/Desktop 1"))

print("== 4. KWin focuses a window of the desktop it switched an output to")
for title in ["w1", "w2"]:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)
script(
    "place",
    """
var d = workspace.desktops;
var w1 = win("w1"), w2 = win("w2");
w1.desktops = [d[0]];
w1.frameGeometry = { x: 100, y: 100, width: 600, height: 400 };
w2.desktops = [d[1]];
w2.frameGeometry = { x: 800, y: 100, width: 600, height: 400 };
workspace.activeWindow = w1;
""",
)
checks.check(
    "w1 has the focus, on the first desktop",
    kde.wait_for(lambda: kde.title(kde.active()) == "w1"),
)
script(
    "to-two", "workspace.setCurrentDesktopForScreen(workspace.desktops[1], workspace.screens[0]);"
)
checks.check(
    "after the first output shows the second desktop, w2, which is on it, has the focus",
    kde.wait_for(lambda: kde.active() != "" and kde.title(kde.active()) == "w2"),
)

print("== 5. activating a window on another desktop of its output switches that output to it")
script("raise", 'workspace.activeWindow = win("w1");')
checks.check(
    "the first output shows the first desktop again",
    kde.wait_for(lambda: shows() == "Desktop 1/Desktop 1"),
)
checks.check("and w1 has the focus", kde.wait_for(lambda: kde.title(kde.active()) == "w1"))

print("== 6. a window a script moves onto the other output keeps its desktops")
script("cross", 'win("w1").frameGeometry = { x: 2300, y: 100, width: 600, height: 400 };')


def where_w1() -> str:
    lines = script(
        f"where-{next(counter)}",
        'var w = win("w1"); report(w.output.name + " " + names(w.desktops));',
    )
    return lines[0] if lines else ""


second = names_of[1] if len(names_of) == 2 else "?"
checks.check(
    f"w1 is on {second} now, and still on the first desktop only",
    kde.wait_for(lambda: where_w1() == f"{second} Desktop 1"),
)

print("== 7. a window put on another desktop from outside says so")
script(
    "watch",
    'var w = win("w1"); w.desktopsChanged.connect(function () { report("moved", names(w.desktops)); });',
    keep=True,
)
kde.sh(f"kdotool set_desktop_for_window {kde.wid('w1')} 2")
checks.check(
    "w1's desktopsChanged was raised, with the second desktop",
    kde.wait_for(lambda: "moved two" in kde.kwin_reports("watch")),
)

kde.stop()
checks.finish()
