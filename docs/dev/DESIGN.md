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

## Why does a maximized window leave the layout, instead of being given the work area?

Because KWin has a maximize of its own, and komorebi asks for it. The window gets the work area of
its output with the panels left alone, and a restore goes back to where the window was
(`docs/kde.md`, and the comment in the bridge where it carries the command out). It is the work
area and not the padded one that [`Workspace::update`](../../komorebi/src/workspace.rs) maximizes
a window to ("the compositor's own maximize, which is the work area and not the padded one"), so
the window cannot be a container: a layout would give it a tile, less the padding. It is held in
`maximized_window` instead, with the index it came from, and when it is restored it goes back to
that index in a container of its own. It is restored before it is tiled again, because "it is its
tile's size that it has to get back, not the compositor's idea of its old size" (the comment in
`reintegrate_maximized_window`).

## Why does one `komorebi.json` work on every platform?

Spike 8 asked whether it could, and it can. With the `StaticConfig` of komorebi for Mac, the
example of komorebi for Windows loaded and read back, and so did a file with every key that only
Windows has, a real file of komorebi for Mac, and this repository's example; what serde dropped
was the keys that mean nothing outside their platform. So a key that does nothing on KDE Plasma is accepted and
ignored, and `config_parity` checks that the keys it ignores are the Windows-only ones and, until
the features are in, the ones of features that are not. Where the Mac struct had no key that the
native mapping needs (`focus_follows_mouse`, `transparency`), the KDE struct has the Windows one.

## Why does komorebi reload its configuration when the file changes?

It does what komorebi for Mac does: it watches `komorebi.json` and reloads it after about two
seconds. On Windows, `watch-configuration` is about the legacy AutoHotkey and PowerShell files,
which do not exist here. The watcher is `hotwatch`, whose default is to wait two seconds for the events of an
editor to settle. An editor either writes a file in place or replaces it with a new one, so the
daemon reloads on a modification and on a removal, and the test that follows a real daemon
through both of them is `the_daemon_follows_the_file_it_is_given_when_the_file_changes`.

## Why is an applications file that is not there skipped, and one that is broken an error?

Because one `komorebi.json` is meant to work on every platform, and the file it points to may be
one that another platform has: a configuration written for macOS names `applications.mac.json`,
which a KDE desktop does not have. The daemon warns and goes on. A file that exists and is not an
applications.json is a mistake in something the user wrote, and is an error.

## Why is a key komorebi does not know a warning, and a value it cannot read an error?

Because one `komorebi.json` is meant to work on every platform, and a key that komorebi for KDE
does not read may be one that another platform does. serde drops a key the types do not have, so
loading never fails because of one, and `komorebic check` does not fail for one either: the keys
only Windows or macOS reads are listed once as a note, and only a key that no platform has is a
warning, with the nearest key that exists, since it is most likely a typo and komorebi will not
have done what was meant. A value of the wrong kind is a different matter: the daemon does not
read the file (a daemon that is running keeps what it had), so the check shows the line and exits
with status 1. A workspace that is only a name is a warning too: it is not tiled, which is easy to
do without meaning to.

## Why is a daemon that runs without its script in KWin an error, and one that does not run a note?

A daemon without its script in KWin cannot see a window, so it tiles nothing while it runs: that
is an error, the one finding of the desktop check that says a daemon which is running is broken.
A daemon that is not running is not broken, so it is a note, with the command that starts it. A
script that is still in KWin when its daemon is gone is a warning, since starting the daemon
replaces it. The check tells which of the three it is by connecting to the daemon's socket and by
asking KWin whether it has loaded `komorebi-bridge`.

## Why does `nix/komorebi-options.nix` start with the hash of the schema?

Because the check that the options are fresh cannot make them again. `schemars-nixgen` makes
`nix/komorebi-options.nix` from `schema.json`, and it is not in the dev shell or in CI (it is
installed with `cargo install`). `just nixgen` writes the hash of `schema.json` as the first line
of the file, and the check `schema-fresh` compares that with the hash of the committed schema, and
the schema with what `komorebic static-config-schema` prints. A change to a key that leaves the
schema or the options behind fails the check, with no tool but the build.

## Why does `komorebic` write an autostart entry, as well as the module having a unit?

The module is NixOS, and the daemon runs on other distributions too. An XDG autostart entry is
what a Plasma session runs at login for a program that is not part of it, so
`komorebic enable-autostart` writes one, with the `komorebi.json` it is given if any. The entry is
ordered after `plasma-workspace` (`X-KDE-autostart-after`), as the module's unit is ordered after
`plasma-core.target`: the daemon exits when KWin is not on the session bus. The entry and the
unit do not go together, since the session bus gives its name to one komorebi.

## Why is KDE Rounded Corners built from source, and not taken from nixpkgs?

Because the version in nixpkgs draws its outline in the wrong place on KWin 6.8, the oldest
KWin komorebi supports. At time of writing (2026-10-27, nixpkgs `d3b13ad`) nixpkgs has
0.10.0, which puts the outline about 24 px too low, across the title bar. 0.11.0 is the
release that handles the texture coordinates KWin 6.8 inverted, so
[`nix/kde-rounded-corners.nix`](../../nix/kde-rounded-corners.nix) overrides the nixpkgs
package with it ([PLATFORM.md](PLATFORM.md#kwin-effects)).

## Why are the effects built disabled by default?

So that they cannot load with their own defaults before komorebi configures them. KWin loads
an effect that is enabled by default as soon as it starts, with the settings the effect has
then, and KDE Rounded Corners at its defaults draws a one pixel black outline over the title
bar of every window. Built with `EnabledByDefault` false, an effect is loaded only when
something asks for it with `loadEffect`, which can come after its settings are written. That
KWin then starts without the effect and loads it when asked was measured before the package
was written ([PLATFORM.md](PLATFORM.md#kwin-effects)). Geometry Change ships disabled by
default, and its check asserts it, so that an update of the effect cannot change it unseen.

## Why does the compositor draw the borders, and not komorebi?

Because the compositor has the frames, the corners and the shadows of the windows, and komorebi
has none of them. komorebi decides what the windows look like and describes it to the backend as a
`Look` ([`Backend::apply_look`](../../komorebi/src/backend/mod.rs)); on KWin the effect KDE
Rounded Corners draws it, and komorebi writes the effect's settings. The same code that decides the
look then runs against the fake desktop, which records the look it was asked for.

## Why is the border the outer outline of the effect?

Because the effect's own defaults draw a black outline one pixel thick *inside* every window, over
its title bar and contents, which is what an unconfigured install looks like. So komorebi sets
every inner outline to 0 and uses the outer outline, which is drawn outside the window, in the gap
the layout leaves between tiles. The effect draws the outer outline only in the shadow of the
window (its shader stops where the window's texture is transparent), so it is at most 20 px thick,
and a window that has no shadow has no border. komorebi does not take the title bar off a window
to change that: removing it also removed the shadow, and the border with it, and Plasma 6.8 gives
a window without a shadow one of its own ([PLATFORM.md](PLATFORM.md#kwin-effects)).

## Why does the layout leave room for the border only while it is drawn?

Because the border is outside the window: without room for it, it would fall on the window next
to it. And because komorebi cannot know that a border is drawn. The effect may not be installed
(KWin finds effects when it starts), or borders may be off, so the backend answers whether
`apply_look` drew one, and the layout leaves room only after a yes. A compositor that draws no
border takes none.

## Why does komorebi ask the effect to read its settings again after it loads it?

Because KWin keeps the copy of `kwinrc` it read when it started, and an effect reads its settings
from that copy, also one loaded a moment ago. Writing the file and loading the effect leaves the
effect with the settings the file had at the start of the session. `reconfigureEffect` makes it
read them again, so komorebi calls it every time, after loading too.

## Why does the border effect take a color for each window?

Because komorebi colors a window by what it is to the layout (the focused window of each monitor,
a stack, a monocle, a floating window, a locked container), and the stock effect knows only "the
active window" and "the others". komorebi's build of KDE Rounded Corners takes a color for each
window over D-Bus, through KWin's name ([PLATFORM.md](PLATFORM.md#the-patches)). Without that
build every kind falls back to the two colors of the look, and komorebi says so once in its log.

## Why does komorebi animate nothing itself?

Because the compositor draws the windows: an animation komorebi made would be a series of frames
asked of KWin, each a command, where the Geometry Change effect animates what is painted, in the
compositor, at the refresh rate of the screen. komorebi tells the effect how long an animation
lasts ([`border_manager::current_look`](../../komorebi/src/border_manager/mod.rs)). That is why
`animation-fps` is accepted and does nothing, and why the frame KWin reports for a window is the
final one from the start of the animation: the effect moves what is painted and not the window
([PLATFORM.md](PLATFORM.md#kwin-effects)).

## Why is cross-fading off?

Because cross-fading the old contents of a window into the new while it changes size is the
effect's default, and it costs a redraw of both. A window going to its place is a move and a
resize, and what the window shows does not change.

## Why are komorebi's animation styles Qt's easing curves?

Because KWin animates with `QEasingCurve`, and a style is a curve KWin has when it is Qt's name
without the "Ease": `EaseInOutSine` is `InOutSine`
([`AnimationStyle::qt_curve`](../../komorebi/src/core/animation.rs)). A custom cubic Bézier is not
one of Qt's curves, and is drawn as an ease out, the kind of curve a window moving to its place
wants. The stock Geometry Change has one curve, an exponential ease out, so komorebi's build of
it reads a `Curve` setting ([PLATFORM.md](PLATFORM.md#the-patches)).

## Why does the window manager keep the opacity it gave each window?

So that the backend is told only what changed. `transparency_manager::apply`
([`transparency_manager.rs`](../../komorebi/src/transparency_manager.rs)) is called after every
event and every message, and most of them change no window's opacity: the window manager keeps
the opacity it gave each window that is not opaque (`WindowManager::opacity_applied`), sends the
difference, makes a window opaque again when it should no longer be see-through (and only if it is
still there), and makes every one opaque when it stops.

## Why is focus follows mouse KWin's own focus policy?

Because KWin has one, and gives the focus to the window under the pointer by itself. komorebi
asks for it by changing the user's `FocusPolicy` and follows the focus, as it follows any change
of focus, and puts the policy back when focus follows mouse is turned off and when it exits
([PLATFORM.md](PLATFORM.md#files-and-directories)). Both implementations komorebi for Windows
names, `Windows` and `Komorebi`, ask KWin for the same thing. Upstream marks the Windows key
end-of-life in favor of masir, a separate program. The code also sets `DelayFocusInterval` to 0,
which is KWin's setting for no delay between the pointer entering a window and the window being
given the focus; why 0 and not the delay KWin has by default is not written down, and needs a
reason.

## Why is a palette kept by the hash of its picture?

Because the name of a picture alone does not say which picture it is: two folders can each have a
`wallpaper.png`, and one wallpaper can be painted over. A palette takes seconds to make for a
large picture, so it is kept, in the user's cache directory (`~/.cache/komorebi`), where it
outlives a login; the runtime directory, where the socket is, does not. The file is named after the
picture, a hash of its path, size and time of last change, and the variant. A palette that cannot
be kept is made again the next time. komorebi for Windows keeps them by file name alone.

## Why does komorebi set the wallpaper when a workspace comes into view?

Because Plasma keeps one wallpaper for each screen, not for each virtual desktop, and a
workspace is a virtual desktop. The shell shows what it was last told, whatever desktop is shown,
so komorebi tells it again as the workspace changes, as komorebi does on Windows and macOS. It does
so wherever a workspace comes into view (its own switches, a pager's, an application raising a
window of another workspace, and the start), since all of them change what the screen should show.
The picture fills the screen, as komorebi's wallpapers do on Windows.

## Why does komorebi ask the shell what the screen shows before it sets a wallpaper?

Because the workspace's wallpaper is asked for every time the workspace comes into view, and the
user may have changed the wallpaper by hand since: a wallpaper that is not the one komorebi wants
is put back, and one that already is, in the same plugin and fill mode, is not touched, so a
switch between two workspaces with the same picture changes nothing on the screen.

## Why is the wallpaper set in the background, and tried again?

Because the shell is another process, which may be slow, and may still be starting when komorebi is
(a session that has just logged in). Switching workspaces never waits for it: the backend notes the
wish for the output and a task on the runtime does the asking, so the request that runs last finds
the newest wish. A shell that is not there is asked again for about half a minute, with longer
waits, and then komorebi says once that it leaves wallpapers alone until the shell answers.

## Why does a wallpaper hand its theme to the borders by a call, and not by a message?

Because on Windows the theme of a wallpaper is sent to komorebi's own socket, and a message to
itself does nothing a call does not do. The call is made before the workspace is shown, so the
colors are the borders' by the time the first window of the workspace is given one.

## Why are the effects patched?

Because the stock effects lack what komorebi needs of them. Each patch leaves the effect
working without komorebi, with the settings the stock effect has:

| Effect | Patched for | Why |
|---|---|---|
| KDE Rounded Corners | a color for each window, set over D-Bus; drawing a window again when its opacity changes | komorebi colors a window by what it is to the layout (the focused window of each monitor, a stack, a monocle, a floating window, a locked container), and the stock effect has one color for the active window and one for the others. It also draws a window from a texture that has the window's opacity in it, made again only when the contents change or the window is resized, so a change of opacity alone would not show |
| Geometry Change | a `Curve` setting, the name of one of Qt's easing curves | komorebi's `animation.style` is an easing curve by name, and its named styles are Qt's curves with "Ease" in front (`EaseInOutSine` is Qt's `InOutSine`); the stock effect has one fixed curve, an exponential ease out, and no setting for it |

The patches are derivative works of the effects and are under the effects' license, the GNU
General Public License version 3 ([PLATFORM.md](PLATFORM.md#the-patches)).

## Why does the end of a drag carry the position of the pointer?

A window dragged onto another tile swaps places with the window there, and one dragged onto
another monitor goes to that monitor. The window manager finds the tile and the monitor by
asking the backend where the pointer is. Only the bridge can read the pointer, so the KWin
backend asks it through the held call, and returns the last position the table has when no
answer comes within 250 ms. The end of a drag is the moment the window manager asks, and the
event that reports it brings the position of the pointer with it, so the last position the
table has is the one that counts.

## Why is a drag of an edge measured from where the window was when it began?

A window is not always on its tile. A client that sizes itself to a grid, such as a terminal that
keeps to whole cells of its font, is a few pixels short of it, and a window with a minimum size
can be larger. Measured against the tile, every drag of such a window would look as if it had
moved the other edges too, and they would be resized as well. The frame the window had when the
drag began is what the user had under the pointer, so the drag is measured from it. KWin reports
the window's frame all through a drag, so the table keeps the frame of the start apart from it,
and the window manager reads that one when it handles the start.

## Why is the end of a resize the frame the user asked for and not the window's frame?

KWin changes the window's frame only once the client has drawn at the new size, so when the user
lets go the frame can be short of where they took the edge
([PLATFORM.md](PLATFORM.md#how-clients-answer-a-frame)). The frame the last step of the drag asked
for is where the user took it. The table keeps it apart from the window's frame, because the window
manager does not handle the end of the drag at once: it may be busy with an earlier event, and the
frame event KWin sends a millisecond after the end changes the frame in the table before the window
manager reads it. The end of the drag has to survive that, so it is not the window's frame. The
frame events go on updating the window's frame, and the retile that ends the drag puts the window on
its tile. A move changes no size, so the end of a move is the frame.

## Why is every step of a drag worked out from where the drag began?

A step says where the user has the edge now, so the drag so far is the distance from the start,
not something to add to the step before. Starting each step, and the end, from the layout as it
was when the user took hold makes the result of a drag the same with its steps as without them,
and a drag that ends where it began puts the neighbors back where they were.

## Why is the window whose edge is being dragged not moved during the drag?

It is the user's until they let go: KWin moves its edge with the pointer, and komorebi puts it on
its tile when the drag ends. A step tiles the workspace to move the neighbors, and so can anything
else that happens during the drag, such as a window that opens. The guard is in
`Window::set_position`, for the windows being resized, so that whatever tiles the workspace leaves
the dragged window alone.

## Why are the steps of a drag thinned out?

The pointer makes far more steps than anyone needs. The bridge sends one every 16 ms at most, the
newest, and a single-shot timer sends the last one of a burst, so the edge's final place is not
lost. Each step says where the edge is now, so one waiting in the window manager's channel is
enough: the window manager asks the window where its edges are when it gets to the step, and that
is the newest.

## Why does the bridge look at the outputs once a second?

Because KWin has no signal for the room a panel leaves, only for the outputs themselves. A
panel that starts, grows, moves or hides changes the work area of an output and raises nothing
the script can connect to, so the bridge reads the work area of every output again when
`screensChanged` is raised and when a dock window opens, closes or moves, and once a second in
its watchdog. Two of the moments are not enough alone: komorebi usually starts before the panel
does, and a panel is laid out after the resolution it follows, so the area at the moment of the
signal is not the final one. The bridge keeps the outputs it last sent and sends them only when
they are not the same, so a second that changed nothing sends nothing. The cost is that a panel
that changes its room with no dock window event is found up to a second late, and that the
bridge does a little work every second.

## Why does komorebi take the windows of an unplugged monitor over itself?

Because nothing else can tile them. When an output goes, KWin takes its windows to the outputs
that are left and puts them where it likes. komorebi has them in the containers of the monitor
that went, whose tiles are on a screen that is gone, so the windows sit untiled wherever KWin
put them. komorebi moves each container to the monitor that now holds the middle of its window,
and tiles the monitors that changed. It keeps the
monitor that went, with its workspaces and settings, so that an output that is plugged in again
gets them back: the windows the monitor had are tiled elsewhere by then, and it comes back
without them.

## Why does komorebi look at every window again a moment after the monitors change?

Because KWin moves windows about for a moment after the outputs change, so what it reports
first is not where the windows end up. An output that went takes its windows to another, one
that is plugged back may take them again, and a change of scale or resolution changes where the
middle of a window is. The monitor reconciliator therefore waits 1.5 seconds after a change of
the monitors (a burst of changes makes one wait) and then finds every container whose window is
on another monitor than the one komorebi has it on, and follows it there. A window that KWin
reports on another output is handled the same way, looking at every window and not only that
one, since when one has moved others may have too. komorebi tiles at once the windows it knows
about when the monitors change, such as those of an output that went; the ones KWin moves
afterwards are tiled up to 1.5 seconds later.

## Why does komorebi look at the desktop again when the machine wakes and the screen is unlocked?

Because nothing komorebi asked for while the screen was locked or the machine asleep was done.
In that time KWin sends its clients no frame callbacks, so a window that was asked to move did
not, a window that something else moved stayed where it was moved, and the monitors can come
back in another order or at another size. Neither an unlock nor a wake is an event about a
window, so komorebi listens for the two signals that say it happened, logind's `PrepareForSleep`
and the lock screen's `ActiveChanged`, and then reads the monitors, finds where every window is,
and puts each in its tile. It waits first: 0.6 seconds after an unlock, because the lock screen
goes before the windows can be seen, and 2.5 seconds after a wake, because the outputs are found
again one after another and the compositor moves windows about as they are. Going to sleep and
locking the screen change nothing, so komorebi does nothing for them.

## Why does the bridge compare the active window with the last one it announced?

Because after the screen is unlocked the window that is active again is not always announced,
and komorebi's idea of the focus would stay the one it had. Once a second the bridge compares the
window KWin has active with the one it last announced, and announces the active one when they
differ. It costs a comparison of two ids a second.

## Why is a window the user maximizes treated like one komorebi maximized?

Because the compositor reports both the same way, and komorebi has to follow it. The comment on the
`MaximizeChange` event says that komorebi's own `toggle-maximize` "does this too and finds the window
already as it asked, but the user's title bar does it without komorebi's knowing". A window that
stayed a tile would be asked to move at the next retile: the code that reads the windows that are
open when komorebi starts says a window that is already maximized is "the maximized window, not a tile
that would be asked to leave the maximized state". So
[`WindowManager::adopt_maximize_change`](../../komorebi/src/window_manager.rs) takes the window out of
the layout as `toggle-maximize` makes a window leave it, and what is asked of the layout does
nothing "until it is restored, by `toggle-maximize` or by the user" (the comment in
`process_command`). Because the report is the same either way, one comparison serves both: a window
that is, or is not, already the workspace's maximized window changes nothing.

## Why does komorebi leave a fullscreen window alone, and tile the workspace when it comes back?

Because the window covers the monitor on purpose. The comment in
[`Window::set_position`](../../komorebi/src/window.rs) says that "moving it would take it out of
fullscreen, or be ignored", and that "it goes back to its tile when it leaves fullscreen". The
window stays in its container, so the other windows are tiled as if it were not fullscreen, and
every move the window manager makes goes through `set_position`, which is where the window is
skipped. When the window comes back, "KWin gives it the frame it had before, which may not be the
tile it has now" (the comment in `handle_event`), so the workspace is tiled again. The bridge's
second request for a position gives up after three tries for the same reason: so that a window that
cannot go where it was asked, "a fullscreen one, say", is left alone rather than chased (the comment
on `settle` in [`bridge.js`](../../komorebi/src/kwin/bridge.js)).

## Why does `initial_window_placement_rules` take a number as well as text?

Because the documentation of the setting shows a number (`"initial_window_placement_rules": 3`),
and a JSON object's keys can only be text: the doc comment on the `Deserialize` of
[`PlacementTarget`](../../komorebi/src/core/mod.rs) says both. The place is read from `"Primary"`,
`"3"` and `3`, so the form the page shows works as well as the form a map's keys force. In komorebi
for Windows, at its commit 83c4e11d, the deserializer reads only text, so the number its page shows
is refused. [The page of this repository](../initial-window-placement-rules.md#on-kde) says which
forms are read.

The JSON schema of the type is written by hand, so that it says the same. The one `schemars`
derives for the enum is an object with a `Placement` or an `Index` key, which the comment on the
`JsonSchema` impl of [`PlacementTarget`](../../komorebi/src/core/mod.rs) calls "not what
`Serialize` and `Deserialize` read and write". The schema `schema.json` has says a name, a number,
or text that is a number, which is also what the module's typed `config` takes.

## Why are the key binding files read by parsers written for komorebi?

All three formats, skhd's, whkd's and sxhkd's, are read by parsers written by hand in
`komorebi-hotkeys`. For whkd there is a Rust parser, `whkd-parser`, but it requires a
`.shell pwsh|powershell|cmd` line, panics in `Shell::from` on any other shell, and its
per-application bindings match Windows process names. It is under the same license as
komorebi, so it stays usable as a dev-dependency: a test reads every whkdrc of the fixture
corpus with both parsers and compares the bindings. For skhd and sxhkd the reason is not
recorded; this entry needs one.

What is not plain syntax is expanded while the file is read: sxhkd's braces become one binding
per alternative, and skhd's `.define` templates and application groups are replaced where
they are used, so the model has only expanded bindings in it.

## Why are some bindings shortcut files, and the others the engine's?

A KDE global shortcut can be a `.desktop` file that KDE reads by itself. Such a static
shortcut is robust: it shows in System Settings, where it can be changed, and it survives a
komorebi process that has died. But it is one combo that runs one command: it cannot be in a
mode, wait for the second key of a chord, fire on release, or do something else in each
application. So a binding is a static shortcut when it is in the default mode, a single combo,
unscoped and a press binding, and the file declares no modes; everything else goes through
the engine, in which komorebi registers the keys itself and decides what each press does. A
blacklist and a pause binding apply to every binding, so a file with either is the engine's
too.

## Why does the engine handle every binding by default?

The plan was to make static shortcuts the default and to switch the default if a static
shortcut turned out to be slow to start its command. It did: on KWin 6.8.1 (in a virtual
machine, on 2026-10-27) a static shortcut ran its command 165 to 436 ms after the key, 233 ms
at the median, while the engine was told of the key 3 ms after it. So the default is
`PlanMode::Engine`, and `PlanMode::Auto` and `PlanMode::Static` stay selectable.

The cost is the robustness of static shortcuts: the engine works only while komorebi runs,
and a komorebi process that was killed leaves its keys registered and dead until they are
released.

Which of the two is the default is still open for a decision. `PlanMode::default()` is the
one place it is set, and `komorebic hotkeys compile` takes its default from there.

## Why does the engine grab and give back keys, instead of using KDE's own key sequences?

Because KDE's do not work for this, as measured on KWin 6.8.1 (2026-10-27). kglobalacceld
accepts a sequence of keys for one action (`Meta+A, B`), but the action never fired: the
second key reached the focused application instead. What does work is grabbing and giving
back single combos: a combo registered with `setShortcutKeys` is swallowed and reported,
also a bare key with no modifier, and after `setInactive` the same key reaches the
application again. Sixty such calls, sent without waiting for the answers, were all answered
in 2.4 ms.

So the engine realizes everything with one set of active combos. A chord chain is a combo
that, when pressed, makes only the combos that can come next (and Escape) active, until one
is pressed or the chord timeout passes. A mode is a different set. A per-application binding
is in the set only while the focused application has an action for it, so that elsewhere
the key reaches the application. After every event the engine computes the set it wants and
returns the difference.

A held key sends one pressed signal, then repeated ones, then one released signal. The
engine acts on the pressed signal only, so holding a key runs its binding once.

The engine itself does no I/O: it returns what to do, and the daemon does it. This entry
needs the reason for that split.
