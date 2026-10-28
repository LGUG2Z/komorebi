# Design

This document explains why komorebi for KDE is built the way it is, one decision at a time.
It is opinionated: these are the reasons behind the code, and reasonable people may disagree
with them. Where a reason was not written down, the entry says that it needs one.

It grows with the code: a decision is added in the commit that makes it.

## What is komorebi for KDE the port of?

komorebi has a Windows implementation, the original, and a macOS one. The Windows one is
the reference. I used it as the reference for what to adapt, and I prefer what KDE and
Wayland do natively over a workaround that the other two platforms need. The events the
window manager reacts to follow the Windows ones (a window is shown, destroyed, focused,
minimized, moved, resized), and a window is a small `Copy` handle, as there. The model the
window manager tiles is the one both platforms have: monitors, each with workspaces, each
with containers, each with windows.

## Why is everything the window manager needs behind one trait?

So that the window manager can be tested without a compositor. A test that needs a real
KWin has to boot a virtual machine, which takes a minute or more and needs QEMU with a
hypervisor ([vm-tests.md](vm-tests.md)); a test against an in-memory desktop takes
milliseconds and runs anywhere. The logic (which window goes where, what happens when one
closes, how a stack cycles) is tested with the second kind, and the VM tests are kept for
what only KWin can tell.

The cost is that a second implementation has to be kept honest. A fake that does what it
is told is not KWin: a real client may refuse a size, a compositor may report an event the
fake never raises, and a fake that is too permissive hides a bug that a compositor would
show. The fake is therefore written to be strict where KWin is (a command about a window
that no longer exists is an error) and to clamp where a real client does (a window with a
minimum size keeps to it).

## Why is the backend a process-wide singleton?

So that a window can be a small `Copy` value that carries only its id. A window that
had to hold a reference to its backend would not be `Copy`, and every function that
moves, focuses or hides one would take a context argument as well. That is a lot of
plumbing for a thing that never changes while the daemon runs.

The cost is a global. A test that wants a private desktop overrides it for its own thread
with `with_backend`, which restores the previous one afterwards, also on a panic.

## Why does a backend answer from a table instead of asking the compositor?

A query that waits for the compositor can wait for a long time, and the window manager
asks a lot of questions: every move of a container is followed by a look at where each
window ended up. A backend keeps a table that the compositor's events update, so a query
is a lookup. The trade-off is that the table can be a moment behind the compositor, which
is why a command returns when it has been accepted and not when the client has acted, and
why the frame a window really has is read back afterwards.

## Why does komorebi not pick the next window to focus when one closes?

Because the compositor already has. When a window is destroyed, KWin activates the window
it considers next and reports that as a focus change, which komorebi follows. If komorebi
also focused a window of its own choosing, the two would fight over the focus. komorebi
focuses the window of its focused container only when the compositor has left nothing
focused.

## Why does komorebi run a script inside KWin?

KWin has no protocol that lets another process read and set window geometry. Its scripting
API has both, and a signal for each change komorebi follows: a window added, removed,
activated, minimized, moved or resized. So the part of the backend that sees and touches
windows is a script KWin runs, the bridge, and the daemon talks to it over the session bus.

## Why does the bridge hold no logic?

So that everything the backend decides can be tested without KWin. A script runs only inside
KWin: its own tests run it against a fake of KWin's API, which is only as right as the fake,
and a test of it against KWin needs a virtual machine. The bridge reports what KWin says, as
it says it, and carries out what it is told; what an event means (is this a window komorebi
manages, has it been announced already, which id does it have) is decided by the table in
`kwin::state`, which is plain Rust that takes the bridge's JSON and is unit-tested with it.

## Why is the bridge type-checked and tested without a JavaScript build chain?

I wanted safety, tests and typing around the bridge that do not need a JavaScript build
chain. KWin loads `bridge.js` as it is, so the checks read that file and produce nothing:

- `tsc` checks it as strict TypeScript through its JSDoc comments (`checkJs`, `noEmit`),
  against `kwin-api.d.ts`, a hand-written declaration of the part of KWin's scripting API the
  bridge uses. KWin publishes no declarations, and the declarations are narrower than the API
  where that catches a mistake: a property the bridge has no reason to set is read-only, and
  `callDBus` takes one string.
- `bridge.test.js` runs under `node --test`, node's own test runner, and loads the bridge into
  a `node:vm` context whose globals are a fake KWin.
- The messages of a session are JSON files that the bridge's tests and the Rust tests both
  read, so neither side can change a message alone.

`tsc` and `node` come from nixpkgs, through the dev shell and the flake's
`komorebi-bridge-js` check. There is no package.json, no npm dependency and no bundler.

## Why do commands go to the bridge as the answer to a held call?

Because it was the fastest channel measured, and it costs nothing while nothing happens. Four
ways for the daemon to get commands to a script in KWin were measured in October 2026, on
KWin 6.8.1 in a VM under HVF: twenty windows retiled in one batch, twenty times, timed from
the send until every window reported its new frame.

| Channel | p50 / p95 / max |
|---|---|
| a held `Poll` call, answered when there are commands | 16 / 19 / 20 ms |
| the script polls on a 16 ms timer | 16 / 20 / 32 ms |
| the script polls on a 50 ms timer | 49 / 57 / 65 ms |
| a new script per batch (`loadScript`, `run`, `unloadScript`) | 17 / 18 / 20 ms |

A timer is as fast only at about 60 D-Bus calls a second, forever. A new script per batch is
as fast, but 7 of its 17 ms (p50) are spent in the daemon's calls to KWin. A held call is
answered as soon as there is a batch, and an idle bridge makes one call every 10 seconds.
Rounds after idle gaps of 15 seconds to 10 minutes all completed, the slowest in 49 ms.

The hold is 10 seconds because QtDBus fails a call after 25 seconds. A call that fails is
never answered (the callback of `callDBus` runs on success only), so the bridge has a
one-second watchdog that calls `Poll` again when the last call is older than the hold plus 3
seconds, which is also how it finds a daemon that has restarted.

## Why are events and the held call on two D-Bus objects?

Because events must arrive in order and a held call waits. zbus handles each method call in a
task of its own by default, and two pairs of events were seen swapped. The `Event` interface
is therefore `spawn = false`: zbus handles its calls inline, one after the other, in the
order they arrive. The price is that a handler that waits holds up every call after it, so
`Event` never waits, and `Poll`, which waits by design, is a separate object with the default
spawning.

## Why does everything cross D-Bus as a JSON string?

Because a script cannot choose the D-Bus type of what it sends. `callDBus` sends a JavaScript
number as a 32-bit integer, and a call whose arguments do not match the method's signature
fails with no error the script can see. One string argument always matches, and the daemon
parses it. The declaration of `callDBus` the bridge is type-checked against takes one string,
so `tsc` refuses anything else.

## Why does the pointer follow the focus through a KWin shortcut?

Because a KWin script cannot move the pointer, and KWin can. Its "Move Mouse to Focus" action
puts the pointer in the middle of the window that has the focus, and it can be invoked over
D-Bus, through `kglobalaccel`: in the spikes it moved the pointer from the middle of the screen
to the middle of the focused window, for two different windows. The backend asks for it once
KWin reports the window it focused as active, so the action finds the right one. The cost is
that the pointer can go to the middle of the focused window and nowhere else, which is what
komorebi asks for.

## Why does komorebi take every window out of its layout when KWin leaves the session bus?

Because nothing else will say that they have gone. When KWin goes away under a running
session (it crashed, or another KWin replaced it), its clients lose their connection and
their windows go with it, and so does the bridge, which would have reported them closing. In
the spikes KWin was killed and started again under a running daemon on the same bus, twice:
the old windows vanished without a single `removed` event. What the daemon does see is the
bus itself: `NameOwnerChanged` says that `org.kde.KWin` has lost its owner, and later that it
has a new one. So losing the name counts as every window closing, and getting it back as the
moment to load the bridge into the new KWin, which then reports its desktop as it does at
start.

## Why does komorebi take its orders on a socket?

komorebi responds to two things: what the compositor reports about windows, and the
messages it receives on its socket. It handles no keyboard or mouse input. Binding a key to
an action is the job of another program, which runs a command that writes a message on the
socket. This is the architecture that bspwm made popular on Linux and yabai on macOS, and
the one komorebi has on Windows and on macOS
([the design notes of komorebi for Mac](https://github.com/LGUG2Z/komorebi-for-mac/blob/master/docs/design.md)).

## Why is `komorebic` a separate program?

Because the daemon takes no input but its socket. `komorebic` turns a command line into one
message on the socket and, for a question, prints the answer. A key binding, a script and a
person at a terminal all drive komorebi the same way, by running `komorebic` (the design
notes of komorebi for Mac, linked above).

## Why does komorebi send notifications to subscribers?

So that a program such as a status bar can follow the window manager. The README of komorebi
for Windows demonstrates a custom status bar integrated through its notifications
([Window Manager Event Subscriptions](https://github.com/LGUG2Z/komorebi#window-manager-event-subscriptions)),
and komorebi for Mac sends the same notifications to Unix sockets, which is also how
`komorebic` talks to its daemon
([its README](https://github.com/LGUG2Z/komorebi-for-mac#window-manager-event-subscriptions)).
A subscriber here is a Unix socket, as on macOS. The named pipes that komorebi on Windows
sends notifications to are not ported, since the socket subscribers do the same.

## Why does the flake have a NixOS module?

So that komorebi runs on NixOS with one flake input and one module line. The module installs
komorebi and starts it with every Plasma session, and it adds the flake's overlay to nixpkgs
itself, so a configuration needs nothing else to find the package. Its options follow those of
the nix-darwin module of komorebi for Mac: `enable`, `package` and `logLevel`.

## Why does komorebi run as a systemd user unit?

Because a user unit covers every user: it is defined once for the machine and runs for each
user who logs in. It is wanted by `plasma-workspace.target`, so it runs in Plasma sessions and
nowhere else, and it is ordered after `plasma-core.target`, which Plasma orders after KWin's
service: the daemon exits when KWin is not on the session bus. systemd starts a daemon that fails
again.

## Why is there no binary cache?

The Komorebi license does not allow third parties to redistribute binaries. There is no public
binary cache for komorebi, and every installation builds it from source.

## Why do the VM tests run on machines built with the module?

So that the tests exercise the module. Every VM suite boots a machine that imports
`nixosModules.default` and installs komorebi through it. The headless node runs the daemon with
the module's read-only `execStart`, and the session node runs the module's unit itself. A change
to the module that breaks how komorebi is installed or started fails the VM suites, and not only
`nixos-module-eval`.

## Why does the bridge ask KWin twice for the position of a window?

KWin moves a window anywhere it is asked to, but when it resizes a window it keeps 100 pixels of
it on an output, and it does so once the client has drawn at the new size, which is after the
bridge has set the frame. The scrolling layout moves the windows of the columns out of view off
the screen and resizes them in the same command, and a terminal that rounds its size to its
cells is resized again by its client, so KWin pulls both back onto the screen. By then, setting
the position alone is a plain move, which KWin honors. The bridge remembers where it was asked
to put a window for a second, and sets the position again when the window's frame changes to
another one in that time ([PLATFORM.md](PLATFORM.md#how-clients-answer-a-frame)). Three tries
at most keep it from chasing a window that cannot go where it was asked, such as a fullscreen
one.

## Why are komorebi's workspaces KWin's virtual desktops?

Because KWin has virtual desktops, per output since KWin 6.8, and Windows and macOS have no API
for them. komorebi's own workspaces exist there for that reason: switching workspace hides
every window of the workspace being left and restores the windows of the one being entered. On
KDE that is what KWin does with its desktops, and native behavior is preferred over a
workaround (see [What is komorebi for KDE the port of?](#what-is-komorebi-for-kde-the-port-of)):
workspace `i` is desktop `i` on every monitor, and each monitor shows one at a time.

The spikes measured what KWin does with per-output desktops on a headless KWin with two
outputs: showing a desktop on one output left the other output's alone; KWin gave the focus to
a window of the desktop it switched to, by itself; and activating a window on another desktop
of its output (an application raising itself) made KWin switch that output to the window's
desktop and focus the window, by itself. So komorebi asks for a switch and follows the ones it
did not ask for, with no command in reply. A first attempt in which komorebi hid the windows
itself switched between two workspaces about 280 times a second after an application raised a
window of another workspace: KWin delivered focus events it had generated before it carried
out komorebi's commands, and komorebi took each for a new request. With KWin's desktops there
is nothing to debounce, since komorebi answers a switch with nothing.

The costs: desktops are global, so every monitor has as many as the monitor with the most
workspaces, with one name each, and they show in the pager and the overview. A window komorebi
does not manage (a dialog, say) belongs to the desktop it opened on and goes out of view with
it. The desktops komorebi created stay when it stops: by then they are the user's desktops.

## Why does komorebi edit `kwinrc` itself?

Because per-output virtual desktops are off by default, they are a setting in the user's
`kwinrc`, and the program that would set it cannot be counted on. KDE's `kwriteconfig6` was not
on the PATH of komorebi's user service in the first live run in a Plasma session, and
per-output desktops stayed off; `kreadconfig6` is not always installed either. The file is
plain ini, which komorebi changes line by line, and asks KWin to reload its configuration.
It is the user's setting, so the value komorebi found is kept in a backup file and put back
when komorebi stops; a backup a crash left behind keeps the value from before the first run,
and the next run puts it back.

## Why are the windows behind the front one of a stack moved to a parking desktop?

Because a container has one tile, so the windows of a stack that are not in front have to be out
of sight, and a virtual desktop nobody looks at was the one way to do that which the spikes found
to cost no focus change. The first spike hid six tiled windows and showed them again three ways.
Minimizing
them gave the geometry back exactly, but KWin picked a new active window as each one minimized,
two focus events nobody asked for. Moving them to a virtual desktop of their own gave no focus
event and the geometry back exactly. Moving them off the screen worked too, since KWin does not
clamp a scripted move, but the windows stay mapped on the current desktop, in the window switcher,
the overview and the task manager, so it was not offered. KWin has nothing else: `Window.hidden`
cannot be set by a script. The spike's source reading, KWin 6.8.1, is in
[PLATFORM.md](PLATFORM.md#the-parking-desktop). The other platforms have a setting that chooses
how windows are hidden; on KDE there is one way, and so no such setting.

The cost is that KWin keeps a virtual desktop in `kwinrc` as soon as it is made. A run that failed
after making the parking desktop left it there for every later session. So komorebi removes the
desktop when it stops, after the commands that bring the windows back have reached the bridge, and
the first snapshot of the next run finds a leftover, sends the windows on it to the first
workspace's desktop and removes it. A leftover is never used as the parking desktop of the new
run, since it is about to be removed and anything parked on it would go with it; the new run makes
its own the first time it needs one. Before each hide the table also checks that the desktop it
made is still listed, because somebody can remove it in System Settings, and a window sent to a
desktop that is not there has nowhere to go and nothing says so.

Activating a window that is on another desktop makes KWin switch that output to the window's
desktop (see [PLATFORM.md](PLATFORM.md#per-output-virtual-desktops)), so a click on a hidden
window in the task manager would leave the monitor showing the parking desktop, which has nothing
on it but hidden windows. The table sends the monitor straight back to the desktop it showed
before, and the window's own focus change then brings it to the front of its stack.

komorebi keeps a table of the windows it hid, [`HIDDEN_WINDOWS`](../../komorebi/src/lib.rs), and
records a window in it before it asks the backend to hide it. What the compositor reports about a
hidden window (a minimize, a move to another desktop) is the echo of a command komorebi sent, and
the window manager has to be able to tell that from the user's own: a window that the user
minimizes leaves the layout, and one komorebi hid does not.

## Why is the monocle container not one of the containers?

Because a layout gives every container a tile, and the monocle container is the one that has the
whole work area. [`Workspace::new_monocle_container`](../../komorebi/src/workspace.rs) takes the
focused container out of the workspace's containers and remembers its index, and
[`Workspace::update`](../../komorebi/src/workspace.rs) puts the window of the monocle container on
the work area and gives the other containers no tile. The container is taken out and put back
with the plain remove and insert of the ring, and not the ones that respect locked containers:
the comments of the code say that a monocle, like a maximize, takes over the whole screen before
it is reinserted at the same index, which is what respecting the indexes of locked containers
needs.

The container keeps its resize adjustment, "because when this container is inevitably
reintegrated, it would be weird if it doesn't go back to the dimensions it had before", and for
the same reason `update` does not shorten the list of adjustments while there is a monocle
container: with two containers, one of them in monocle, the list would be cut to one, and the
adjustment of the container in monocle would be lost when it came back (the comments in
`new_monocle_container` and `update`).
