# Resizing a tile by dragging its edge, in a Plasma session, with a pointer device the test drives
# from inside the guest (`komorebi_vm.Pointer`). What only a real KWin and a real pointer can tell is in
# `komorebi_vm.platforms.kde.extras.drag_resize`: edges and corners of three tiles are grabbed and
# dragged, and what happened is checked against what a user expects. The shared scenarios
# (`resize`, `focus_follows_mouse`) run on Windows too and make one drag and the focus changes.
from komorebi_vm import Checks, Kde, Pointer
from komorebi_vm.core.platform import Client
from komorebi_vm.core.scenarios import focus_follows_mouse, resize
from komorebi_vm.core.suite import platform_only, run_suite, shared
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.extras import drag_resize as kde_drag_resize

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)

# KWin snaps an edge the user drags to an edge of another window within its snap zone, 10 pixels
# by default, and the neighbors that follow a drag are about that far from the edge: it would end
# up a few pixels past the pointer. The test measures komorebi, so KWin snaps nothing.
for zone in ("BorderSnapZone", "WindowSnapZone", "CenterSnapZone"):
    kde.sh(f"kwriteconfig6 --file kwinrc --group Windows --key {zone} 0")
kde.sh("busctl --user call org.kde.KWin /KWin org.kde.KWin reconfigure")

# the session started the module's unit when it logged in
checks.check(
    "komorebi answers",
    kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=120),
)

# The windows tiled before the daemon has the panel's room are put in the room that was there, and a
# case that begins then ends with every tile moved by the panel. The wait is not a check: a session
# may never take the room, and it ends after a while.
kde.wait_for_panel_room()

size = kde.state()["monitors"]["elements"][0]["size"]
W, H = size["right"], size["bottom"]
pointer = Pointer.start(machine, (W, H), scale=SCALE)


for n in range(1, kde_drag_resize.WINDOWS + 1):
    kde.spawn(f"foot -T win{n} sleep 3000")
    kde.wait_for(lambda n=n: kde.managed() == n, timeout=30)
checks.check(
    f"the {kde_drag_resize.WINDOWS} windows are tiled",
    kde.managed() == kde_drag_resize.WINDOWS,
)

platform = KdePlatform(kde, (W, H), pointer)
clients = [Client(f"win{n}") for n in range(1, kde_drag_resize.WINDOWS + 1)]
args = (platform, checks, clients)
run_suite(
    checks,
    platform_only("kde", kde_drag_resize.pointer_is_exact, *args),
    platform_only("kde", kde_drag_resize.edge_cases, *args),
    shared(resize.run, platform, checks, clients),
    shared(focus_follows_mouse.run, platform, checks, clients[0], clients[2]),
)

checks.finish()
