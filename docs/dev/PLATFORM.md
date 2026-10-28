# Platform

komorebi for KDE runs in a KDE Plasma session, as a client of KWin. Most of the details on
this page are also written down next to the code that depends on them; this page
centralizes them: which KWin it needs, how it finds it, what it creates in the session and
leaves behind, every environment variable it reads, and what happens when something is
missing.

Facts that change with KWin and Plasma releases are dated. Unless a fact says otherwise, it
was measured with KWin 6.8.1, Plasma 6.8.1 and Qt 6.12.0 from nixpkgs `d3b13ad`
(2026-10-24), in NixOS virtual machines under QEMU on an Apple silicon Mac (HVF).

## Vocabulary

- **Session**: one user's Plasma login: KWin, the Plasma shell, and a D-Bus session bus they
  share.
- **Session bus**: the D-Bus bus of the session. komorebi and KWin talk over it.
- **KWin**: the compositor and window manager of Plasma, `kwin_wayland`.
- **Script**: a JavaScript program that KWin runs inside itself, through its scripting API.
  komorebi's is the **bridge**, [`bridge.js`](../../komorebi/src/kwin/bridge.js).
- **Output**: KWin's name for a monitor.
- **Desktop**: one of KWin's virtual desktops. Each output shows one; komorebi's workspace `i`
  is desktop `i`.
- **Parking desktop**: the desktop named `komorebi-park`, which komorebi makes to hide the back of
  a stack, and the windows behind a monocle, on. It is not a workspace.
- **Headless KWin**: `kwin_wayland --virtual`, a KWin with virtual outputs and no display,
  which the VM tests run ([vm-tests.md](vm-tests.md)).

## What komorebi needs

### KWin 6.8 or newer

KWin 6.8 is the oldest KWin komorebi supports. komorebi's workspaces are KWin's virtual
desktops, one shown on each output, which needs the per-output virtual desktops KWin 6.8
introduced (`workspace.currentDesktopForScreen`, `workspace.setCurrentDesktopForScreen`, and
the setting `[Windows] PerOutputVirtualDesktops` in `kwinrc`). At time of writing (2026-10-27)
KWin 6.8.1 is the only version komorebi has been run against; 6.7 and older have not been
tried.

### Per-output virtual desktops

What komorebi relies on KWin to do with them, on KWin 6.8.1 at time of writing (2026-10-27),
measured in the spikes and checked by the VM test `vm-kwin-contract` on a headless KWin with two
outputs:

- With `PerOutputVirtualDesktops=true` in the `[Windows]` group of `kwinrc`, a script that
  shows a desktop on one output (`workspace.setCurrentDesktopForScreen`) leaves the other
  output's desktop as it was. komorebi turns the setting on when it starts, has KWin reload its
  configuration (`org.kde.KWin.reconfigure`), and puts the user's value back when it stops.
- KWin raises `workspace.currentDesktopChanged` when an output shows another desktop. The signal
  does not say which output, so the bridge reads the desktop of every output after it.
- KWin gives the focus to a window of the desktop it switched an output to, by itself.
- Activating a window that is on another desktop of its output switches that output to the
  window's desktop and gives the window the focus.
- A window that a script moves so that its middle is on another output keeps its desktops;
  `window.output` follows the middle. KWin puts a window on the new output's desktop only during
  an interactive move.
- Putting a window on another desktop from outside the script raises the window's
  `desktopsChanged`.
- Desktops are global: every output has the same desktops, with the same names, and each output
  shows one of them. The ids of the desktops are UUIDs; the bridge names them by id.

### The parking desktop

Why komorebi hides windows on a desktop of its own is in
[DESIGN.md](DESIGN.md#why-are-the-windows-behind-the-front-one-of-a-stack-moved-to-a-parking-desktop).
What it relies on KWin for, at time of writing (2026-10-27), measured by the spikes on KWin
6.8.1 (nixpkgs `d3b13ad`, 2026-10-24) unless it says otherwise. The VM test `vm-stacks` runs
komorebi's use of them in a headless KWin:

- A window on another desktop of its output is out of sight, and moving it there and back gives
  its frame back exactly. Hiding six windows this way raised no `activated` event; minimizing
  them raised two.
- KWin saves a virtual desktop in `kwinrc` as soon as it is made. A desktop that komorebi made
  and did not remove, because it was killed, is in every later session.
- Activating a window that is on another desktop of its output switches that output to the
  window's desktop (see [Per-output virtual desktops](#per-output-virtual-desktops)), which for a
  hidden window is the parking desktop.
- `Window.hidden` is read-only to a script, and the only thing in KWin's 6.8.1 sources that sets
  it is a layer-shell window (an auto-hide panel). This is a reading of the sources, not a
  measurement.
- A window on the parking desktop is still a window of the session. Whether Plasma's task manager
  lists it depends on the task manager's setting for showing the windows of the current desktop
  only; the VM tests have no panel, so this is not measured.

### Wayland

komorebi runs in a Plasma Wayland session. There is no X11 session of Plasma 6.8 to run it in.
Programs that run on XWayland inside the Wayland session are windows like any other; KWin
reports their X11 class (`XTerm`) where a Wayland client has its app id (`foot`,
`org.kde.konsole`).

### The session bus

komorebi finds KWin on the session bus, and nowhere else. It needs to own the name
`org.komorebi.Bridge` there, and KWin has to own `org.kde.KWin`. See
[`DBUS_SESSION_BUS_ADDRESS`](#dbus_session_bus_address) for how the bus is found.

## Environments

The same questions, for each place the daemon can be started:

1. How it finds KWin.
2. What it does there.
3. What it leaves behind.

### A Plasma session

#### How komorebi finds KWin

Through the session bus. When the daemon is started from the session (a terminal, an
autostart entry, a user unit of the session), the bus is already in its environment.

On NixOS, the module's user unit, `komorebi.service`, is wanted by `plasma-workspace.target`
and ordered after `plasma-core.target`, which Plasma orders after KWin's service,
`plasma-kwin_wayland.service`: the session starts the daemon once KWin's service has started
([installation.md](../installation.md)). The unit's `PATH` is `/run/wrappers/bin`,
`/etc/profiles/per-user/<user>/bin` and `/run/current-system/sw/bin`.

#### What komorebi does

It connects to the session bus as `org.komorebi.Bridge`, writes the bridge to
`$XDG_RUNTIME_DIR/komorebi/bridge.js`, and asks KWin to load it under the name
`komorebi-bridge`, replacing a script of that name left by an earlier run. The script reports
the windows and outputs it finds, and from then on every window that opens, closes, takes the
focus, is minimized or brought back, moves or changes size, the start and the end of every drag
of a window by the user, and every change of the outputs (one that changes resolution or scale,
and the room a panel leaves). KWin has no signal for that room, so the script looks at the
outputs again when a dock window opens, closes or moves, and once a second. The daemon waits up
to 10 seconds for the first report and does not start without it.

Before it loads the script, it turns on KWin's per-output virtual desktops in the user's
`kwinrc` (`[Windows] PerOutputVirtualDesktops=true`), unless they are on, and asks KWin to
reload its configuration. If a workspace has no desktop, it creates the missing desktops over
`org.kde.KWin.VirtualDesktopManager`, named "Workspace 1", "Workspace 2" and so on. A desktop
that is already there keeps its name.

When komorebi hides a window for the first time (the back of a stack, or a window behind a
monocle), it makes one more desktop, named `komorebi-park`, after the desktops of the workspaces
(the missing workspace desktops are made by position, so that it stays last), and moves the window
there. The desktop is not a workspace: komorebi counts the workspaces without it, and a window on
it is on the workspace it will come back to. It checks that KWin still lists the desktop before it
sends another window there.

When borders are on, it also asks KWin to load KDE Rounded Corners, the effect that draws them,
after writing the effect's settings into `kwinrc`, and asks it to read them again
([KWin effects](#kwin-effects)). It does the same with Geometry Change, the effect that animates
windows, when animation is on. An effect KWin did not find is reported once in the log. When focus
follows mouse is turned on, it sets KWin's focus policy in `kwinrc` (`[Windows] FocusPolicy=1`,
`DelayFocusInterval=0`) and asks KWin to reload its configuration.

Every command of the window manager (move and resize, focus, raise, hide or restore, minimize,
bring back, maximize, unmaximize or close a window, show a workspace on an output, put a window
on a workspace) is queued for the bridge, which gets the queue as the answer to its held call and
carries the commands out in KWin, in order. A maximize is KWin's own, `setMaximize(true, true)`:
the window gets the work area of its output with the panels left alone, komorebi sets no frame
for it meanwhile, and unmaximizing it gives it back the frame it had. The VM test `vm-features`
checks that the window fills the work area and is on top. The script API cannot move the pointer,
so when the pointer follows the focus komorebi invokes KWin's own "Move Mouse to Focus" shortcut,
which puts the pointer in the middle of the window KWin has focused.

#### When KWin restarts

KWin can go away while the session goes on: it crashed, or `kwin_wayland --replace` took its
place. The session bus stays, and the windows go with the old KWin. komorebi keeps running. It
watches who owns `org.kde.KWin` on the session bus, and when nobody does it takes every window
of the old KWin out of its layout, as if each had been closed: the bridge went with KWin, so
nothing reports them closing. When the name has an owner again, komorebi loads the bridge into
the new KWin, trying again for up to 15 seconds while KWin is not ready for it, and the bridge
reports the new KWin's windows and outputs as it does at start. A window that opens in the new
KWin is tiled. In the spikes the bridge was loaded again 1.0 to 1.1 seconds after the name had
an owner again; the VM test `vm-kwin-restart` restarts KWin twice under a running daemon.

#### What komorebi leaves behind

When it is stopped with `SIGINT`, `SIGTERM` or a `Stop` message on its socket, it brings back
the windows it hid, waits until the commands that do so have reached the bridge, removes the
parking desktop, unloads the effects it loaded, puts back the values the keys it set in `kwinrc`
had (`PerOutputVirtualDesktops`, the focus policy, and the keys of the effects), or removes a key
the user had not set, and asks KWin to reload its configuration, asks KWin to unload the script,
closes an empty connection to each subscriber, and removes the socket. The desktops it created for
workspaces stay: they are the user's desktops from then on. A `StopIgnoreRestore` message stops it
the same way, except that it does not bring the windows back: the parking desktop is removed all
the same. The file `bridge.js` stays in the runtime
directory, which is removed at logout. A daemon that is killed leaves the script running in
KWin, reporting to nobody, the socket file in the runtime directory, per-output desktops
on, and the parking desktop with the hidden windows on it, until the next start replaces the
script and the socket, sends the windows on the parking desktop to the first workspace's desktop
and removes the desktop, and its stop puts the setting back (the backup in the runtime
directory keeps the user's value), or the session ends.

### A headless KWin

`kwin_wayland --virtual` behaves like the KWin of a session for everything komorebi does:
the VM tests run the daemon against one, on a private session bus of its own. It cannot draw
(see [Rendering](#rendering-in-virtual-machines-and-over-rdp)), so nothing that depends on
pixels can be tested against it. Started with `--no-global-shortcuts`, as the VM tests start
it, KWin does not own `org.kde.kglobalaccel`, and the pointer does not follow the focus; the
daemon logs that at debug level and carries on.

### No KWin

Anywhere else (a session that is not Plasma, a Linux console, a Mac), the daemon exits with an
error that names what is missing:

- "could not connect to the session bus as org.komorebi.Bridge": there is no session bus at
  the address the environment gives, or another komorebi owns the name.
- "KWin is not on the session bus": the bus is there and KWin is not.
- "KWin loaded the bridge script but it never reported in": KWin is there, and the script did
  not run. KWin logs a script's errors to the journal (`journalctl --user -t kwin_wayland`).

`komorebi --fake-backend` runs the window manager against an in-memory desktop instead, which
works anywhere, for development and testing.

## D-Bus

What komorebi owns, on the session bus:

| Name | Object | Interface | Member | Called by |
|---|---|---|---|---|
| `org.komorebi.Bridge` | `/org/komorebi/Bridge` | `org.komorebi.Bridge` | `Event(s)` | the bridge, for every event |
| `org.komorebi.Bridge` | `/org/komorebi/Bridge/Poll` | `org.komorebi.Bridge.Poll` | `Poll(s) -> s` | the bridge, for commands |

Both take a JSON string. `Poll` is held: the daemon answers it when it has commands for the
bridge, or with an empty list after 10 seconds, and the bridge then calls it again.

What komorebi calls:

| Name | Object | Interface | Member | When |
|---|---|---|---|---|
| `org.kde.KWin` | `/Scripting` | `org.kde.kwin.Scripting` | `unloadScript(s)`, `loadScript(ss) -> i` | at start, and when KWin is back after a restart; `unloadScript` again at exit |
| `org.kde.KWin` | `/Scripting/Script<id>` | `org.kde.kwin.Script` | `run()` | after `loadScript` |
| `org.freedesktop.DBus` | `/org/freedesktop/DBus` | `org.freedesktop.DBus` | `AddMatch(s)`, for the signal `NameOwnerChanged` | at start, to see KWin leave the bus and come back |
| `org.kde.kglobalaccel` | `/component/kwin` | `org.kde.kglobalaccel.Component` | `invokeShortcut(s)`, with `MoveMouseToFocus` | when the pointer follows the focus |
| `org.kde.KWin` | `/Effects` | `org.kde.kwin.Effects` | `isEffectSupported(s)`, `isEffectLoaded(s)`, `loadEffect(s)`, `unloadEffect(s)`, `reconfigureEffect(s)`, with `kwin4_effect_shapecorners` and `kwin4_effect_geometry_change` | when the look changes (the border is turned on, off or changed), and at exit |
| `org.kde.KWin` | `/org/komorebi/BorderColours` | `org.komorebi.BorderColours1` | `SetColours(asau)`: the UUID of each window and its color, `0xRRGGBB` | when the color of a window's border changes; the object is komorebi's build of the border effect ([The patches](#the-patches)) |
| `org.kde.KWin` | `/KWin` | `org.kde.KWin` | `reconfigure()` | after komorebi changed `kwinrc`, at start and at exit |
| `org.kde.KWin` | `/VirtualDesktopManager` | `org.kde.KWin.VirtualDesktopManager` | the properties `desktops` and `count`, `createDesktop(us)`, `setDesktopName(ss)`, `removeDesktop(s)` | when a workspace has no desktop, and at start; `createDesktop` also for the parking desktop, the first time a window is hidden, and `removeDesktop` for it when komorebi stops and for one a killed run left |
| `org.kde.plasmashell` | `/PlasmaShell` | `org.kde.PlasmaShell` | `evaluateScript(s) -> s`, `wallpaper(u) -> a{sv}`, `setWallpaper(sa{sv}u)` | when a workspace with a wallpaper comes into view ([The Plasma shell](#the-plasma-shell)) |

The property `desktops` is introspected as `a(iss)`, a position, an id and a name for each
desktop, and the value on the wire is `a(uss)`: komorebi reads it as that. It is read uncached,
since a read right after `createDesktop` has to see the new desktop.

## Files and directories

| Path | What | Lifetime |
|---|---|---|
| `$KOMOREBI_CONFIG_HOME/komorebi.json` (`~/.config/komorebi/komorebi.json`) | komorebi's configuration, which komorebi never writes: read when the daemon starts, and again when it changes. `--config` names another file | the user's |
| `$XDG_CONFIG_HOME/autostart/komorebi.desktop` (`~/.config/autostart/komorebi.desktop`) | the XDG autostart entry `komorebic enable-autostart` writes: `Exec=komorebi`, or `komorebi --config "FILE"`, and `X-KDE-autostart-after=plasma-workspace`, which a Plasma session starts komorebi from at login | the user's; `komorebic disable-autostart` removes it |
| `$XDG_CONFIG_HOME/kwinrc` (`~/.config/kwinrc`) | KWin's configuration: komorebi sets `PerOutputVirtualDesktops=true` in `[Windows]`, changing that line or adding it and nothing else, and writes the file beside it first and renames it over it | put back as it was when the daemon stops |
| `$XDG_CONFIG_HOME/kwinrc`, group `[Windows]`, keys `FocusPolicy` and `DelayFocusInterval` | `1` and `0` while focus follows mouse is on: KWin's "focus follows mouse" policy, which KWin takes when it is asked to reconfigure | put back, without the other keys of the group, when focus follows mouse is turned off, and with everything else when the daemon stops |
| `$XDG_CONFIG_HOME/kwinrc`, group `[Round-Corners]` | the settings of the border effect, written when borders are on ([KWin effects](#kwin-effects)) | put back, with the rest of `kwinrc`, when borders are turned off and when the daemon stops |
| `$XDG_CONFIG_HOME/kwinrc`, group `[Effect-kwin4_effect_geometry_change]` | the settings of the animation effect, written when animation is on | put back as above, when animation is turned off and when the daemon stops |
| `$XDG_RUNTIME_DIR/komorebi/kwinrc-backup.json` | the value each key of `kwinrc` had before komorebi first changed it | removed when the values are put back; one a crash left behind is used by the next run |
| `$XDG_RUNTIME_DIR/komorebi/bridge.js` | the script KWin loads, with its configuration (the D-Bus names and the hold time of `Poll`) on its first line | written at every start; the directory is removed at logout |
| `$XDG_RUNTIME_DIR/komorebi/komorebi.sock` | the command socket | bound once the daemon is connected to KWin, after a file left there is removed; removed when the daemon stops |
| `$XDG_RUNTIME_DIR/komorebi/NAME` | the socket of a subscriber, which the subscriber binds and names in its subscription | removed by the daemon, with the subscriber, when it cannot connect to it |
| `$XDG_CACHE_HOME/komorebi` (`~/.cache/komorebi`) | the palettes made from wallpapers, a file for each picture and variant, `NAME-HASH.base16.VARIANT.json` | never removed by komorebi; made again when the picture's path, size or time of last change differ |
| `$XDG_DATA_HOME/komorebi/komorebi.log.YYYY-MM-DD` | the daemon's log, with colors, which `komorebic log` follows | a file a day (the UTC date), never removed by komorebi |
| `$XDG_DATA_HOME/komorebi/komorebi_plaintext.log.YYYY-MM-DD` | the same log without colors | as above |

Without `XDG_RUNTIME_DIR` the bridge and the socket are in `komorebi` in the local data
directory instead, with the logs (see [`XDG_DATA_HOME`](#xdg_data_home)).

## Environment variables

### `DBUS_SESSION_BUS_ADDRESS`

The address of the session bus. If it is not set, komorebi uses `$XDG_RUNTIME_DIR/bus`, where
systemd puts the session bus, and `/run/user/<uid>/bus` when `XDG_RUNTIME_DIR` is not set
either. An address with no bus behind it is the first error under [No KWin](#no-kwin).

### `KOMOREBI_CONFIG_HOME`

The config home, where komorebi looks for `komorebi.json`: a directory, in which `komorebic
configuration` and the daemon look for the file. Without it the config home is
`~/.config/komorebi`, whatever `XDG_CONFIG_HOME` is. When it is set and is not a directory, the
daemon and `komorebic` panic when they first need the config home, and say so.

### `XDG_RUNTIME_DIR`

The runtime directory of the user, normally `/run/user/<uid>`: komorebi writes the bridge to
the `komorebi` directory in it, and binds its command socket there.

### `XDG_CONFIG_HOME`

Where the user's `kwinrc` is: `$XDG_CONFIG_HOME/kwinrc`, or `~/.config/kwinrc` when it is not
set. Without either, komorebi leaves per-output virtual desktops as they are and logs that it
found no configuration directory.

### `XDG_CONFIG_DIRS`

Where KConfig looks for the system's `kwinrc` when the user's has no value: komorebi reads
`PerOutputVirtualDesktops` the same way, from the user's file and then from the first of these
directories that has it (`/etc/xdg` when the variable is not set), and changes nothing when the
setting is on already.

### `XDG_DATA_HOME`

The logs are written to `$XDG_DATA_HOME/komorebi` (`~/.local/share/komorebi` if
`XDG_DATA_HOME` is not set), which the daemon makes before anything else: a home it cannot
write to stops it at once. When `XDG_RUNTIME_DIR` is not set, the bridge and the socket go
there too.

### `KWIN_EFFECTS_FORCE_ANIMATIONS`

KWin's own variable, read by KWin and not by komorebi. With software rendering, which is all a
virtual machine has, KWin loads no effect that animates, Geometry Change included, unless it is
`1` in KWin's environment: at time of writing (2026-10-27) a guest loaded 11 effects without it and
29 with. The VM nodes set it ([Rendering](#rendering-in-virtual-machines-and-over-rdp)); a machine
with a GPU needs nothing.

### `RUST_LOG`

Which log lines the daemon prints, in the syntax of `tracing-subscriber`'s `EnvFilter`
(`komorebi=debug`, `komorebi::kwin=trace`). If it is not set, `--log-level` sets it to
`komorebi=<level>`; `--log-level` defaults to `info`.

### `RUST_LIB_BACKTRACE`

Whether an error carries a backtrace. komorebi sets it to `1` when it is not set.

### `NO_COLOR`

When set to anything but the empty string, the log the daemon prints on its standard output
has no ANSI colors, which is what standard output redirected to a file wants. The VM tests
set it. The log files are not affected: one has colors and the other none.

## The autostart entry

At time of writing (2026-10-27), on a Plasma session of a NixOS machine, systemd's autostart
generator makes a user unit of the entry `komorebic enable-autostart` writes,
`app-komorebi@autostart.service`, when the user manager reloads (`systemctl --user
daemon-reload`), and starting that unit starts a daemon that follows the file the entry gave it
(`vm-module`, which does this on x86_64). The test starts the unit by hand, since the session it
runs in has logged in already: it does not show that the next login starts it.

## Watching the configuration

The daemon watches `komorebi.json` with `hotwatch` 0.5, over `notify` 6.1, which on Linux is
inotify. At time of writing (2026-10-27) a change to the file reaches the daemon about two
seconds later, the delay of `hotwatch`'s debouncer, whether an editor wrote the file in place or
renamed a new file over it: `the_daemon_follows_the_file_it_is_given_when_the_file_changes` does
both, on aarch64 Linux. A file the daemon cannot read after a change is logged and leaves the
configuration it has.

## The bridge's environment

What the KWin scripting API offers a script, and what it does not, at time of writing
(2026-10-27). The bridge is written around these, and
[`kwin-api.d.ts`](../../komorebi/src/kwin/kwin-api.d.ts) declares the part of the API it
uses, which `tsc` checks it against with ECMAScript 2016's library and nothing else. Node and
TypeScript are needed only for those checks: in the session the bridge needs KWin alone.

- A window's `opacity` is a number from 0 to 1 that a script may set: `Window.opacity` is KWin's
  own property, so komorebi sets the opacity of a window by setting it, and the effect that
  draws the border has to draw the window again when it changes
  ([The patches](#the-patches)).
- `callDBus` sends a JavaScript number as a 32-bit integer, and a call whose arguments do not
  match the method's signature fails without a word. The bridge sends every argument as a
  JSON string.
- The callback of `callDBus` is called on success only. A call that fails (the daemon has
  exited) is never answered, so the bridge re-issues `Poll` from a one-second `QTimer` when
  the last one is older than the hold time plus 3 seconds.
- There is a `QTimer`, but no `setTimeout`, and no `Qt` global: a rectangle is assigned as a
  plain `{x, y, width, height}` object.
- KWin activates a new window before it announces it (`windowActivated` comes before
  `windowAdded`). The bridge announces a window it has not seen when it is activated, so that
  no event names a window before the event that announces it.
- A script's `print()` goes to KWin's log, which shows it only when the `kwin_scripting`
  logging category is enabled (`QT_LOGGING_RULES="kwin_scripting.debug=true"`, as the VM
  tests do).
- A drag of a window by the user is `interactiveMoveResizeStarted` on the window, then a
  `frameGeometryChanged` for every frame the pointer moves it by, then
  `interactiveMoveResizeFinished`. While it goes on, the window's `move` or `resize` is true,
  which tells the two kinds apart. A drag injected as pointer input in a KWin 6.8 session gave
  the start, 25 geometry changes and the finish.

## How clients answer a frame

KWin applies a frame komorebi asks for at once, and then the client decides what size it
draws at; the frame KWin reports afterwards (a `geometry` event) is the one the client
chose. Measured at time of writing (2026-10-27) in a headless KWin with eight clients, by the
VM test `vm-client-geometry`, and by the spikes before it with more sizes and at a scale of
1.25:

| Client | Answer | In `vm-client-geometry` |
|---|---|---|
| Konsole, Kate, Dolphin (Qt, KDE) | exactly the frame asked for, also below the minimum size they report (spikes) | Kate asked for 948x527, has 948x527 |
| `xterm` on XWayland | exactly the frame asked for | asked for 229x259, has 229x259 |
| `foot` | sizes itself down to whole cells of its font | asked for 1905x2140, has 1902x2129 |
| GTK 4 Text Editor | exactly the frame asked for, down to its minimum of 360x200 | asked for 469x528, has 469x528 |
| GTK 4 Calculator | never below its minimum of 360x616 | asked for 469x259, has 469x616 |
| Firefox | never below its minimum width of 500 and height of 120 | asked for 230x259, has 500x259 |

Every client keeps the position it is asked for, also one partly off the screen. A fractional
scale of 1.25 gives the same answers as 1.0: every whole logical frame comes back as it was
asked for. komorebi does not ask again when a client keeps to a size of its own: the window
manager reads the frame the window has from the table, and a window larger than its tile
covers part of its neighbor's.

KWin moves a window anywhere it is asked to, off the screen too, but when it *resizes* a window
it keeps 100 pixels of it on an output, and it does that once the client has drawn at the new
size. A window that is moved off the screen and resized at once, as the scrolling layout does
with the columns out of view, is pulled back onto the screen a moment later; so is one whose
client rounds the size, as `foot` does. The bridge therefore remembers the position it was asked
for, for a second, and when the window's frame changes to another position in that time, asks
for the position again, which is a plain move. It tries three times at most, so a window that
cannot go where it was asked is left alone. A maximize makes the bridge forget the position: the
window's frame then changes to the work area, which is not where it was last asked to be. The VM
test `vm-scrolling` checks that the columns out of view stay where komorebi put them.

The same holds for a frame the user asks for. While the user drags an edge of a window, KWin reports
at every step of the drag the frame the pointer asks for (`interactiveMoveResizeStepped`), and the
window's frame follows once the client has drawn at that size. When the user lets go, the frame can
be well short of where they took the edge: at time of writing (2026-10-27), in the spike, the frame
at the end of a 72-pixel drag of a terminal's edge had moved 12 or 48 pixels. KWin goes on reporting
the client's frames after the end of the drag: in a run of `vm-drag-resize` at time of writing
(2026-10-28), a frame of 660 pixels, which is what `foot` had drawn of the 672 asked for, came 1 ms
after the end, and the window manager, busy with an earlier event, handled the end 17 ms after that.
The bridge sends the frame of the last step with the end of the drag, and the end of a resize is
that frame, which the daemon keeps apart from the frames that follow. It also passes the steps on,
one every 16 ms at most, so that the tiles next to the edge follow it while it is dragged; the VM
test `vm-live-resize` reads them while the button is still down. KWin's window snapping applies to
the edge the user drags, and the tiles that follow it are close to it: at time of writing
(2026-10-27), in `vm-drag-resize` with the default paddings (a gap of 10 pixels between two tiles)
and KWin's default snap zone of 10 pixels, a dragged edge once ended 9 pixels past the pointer. The
drag VM tests set KWin's snap zones to 0. The VM test `vm-drag-resize` drags edges in a Plasma
session with a pointer device that the session node makes with uinput.

## Outputs that change

Measured by the VM test `vm-hotplug` at time of writing (2026-10-27), in a headless KWin 6.8.1
with two outputs of 1920x1080:

- `kscreen-doctor output.<name>.scale.1.5` gives an output the geometry 1280x720. That is what
  the script reports and komorebi tiles in: every coordinate komorebi uses is KWin's logical
  one. At that scale `kdotool` prints window frames in fractions of a pixel (a width of
  695.3333), which the VM library rounds.
- `kscreen-doctor output.<name>.disable` is how a headless KWin loses an output, and `enable`
  how it gets it back. The script reports the outputs that are left after the first and all of
  them after the second, and komorebi follows both. A disabled output is enabled again by the
  name it had.

## How long a retile takes

Measured by the VM test `vm-latency` at time of writing (2026-10-27), in a headless KWin 6.8.1
under HVF on an Apple silicon Mac, with ten `foot` windows on a 3840x2160 output: twenty
retiles (a window minimized and brought back, ten times over), each of which moved 5.5 windows
on average. From the daemon's answer to `Poll` to the last frame KWin reported for the windows
the batch moved, the client's answer included:

| p50 | p95 | max |
|---|---|---|
| 17.3 ms | 27.7 ms | 31.4 ms |

The spikes measured the channel alone, with twenty windows in each batch, at 16, 19 and 20 ms
([DESIGN.md](DESIGN.md#why-do-commands-go-to-the-bridge-as-the-answer-to-a-held-call)).

## KWin effects

An effect is a plugin of KWin's compositor that changes how windows are drawn. The flake
builds the effects komorebi drives as packages of their own, from source, with komorebi's
patches (see [The patches](#the-patches)):

| Package | Effect | Plugin id | Installed as |
|---|---|---|---|
| `kde-rounded-corners-komorebi` | [KDE Rounded Corners](https://github.com/matinlotfali/KDE-Rounded-Corners) 0.11.0, which draws an outline around each window | `kwin4_effect_shapecorners` | a C++ plugin, `lib/qt-6/plugins/kwin/effects/plugins/kwin4_effect_shapecorners.so` |
| `kwin-effect-geometry-change-komorebi` | [Geometry Change](https://github.com/peterfajdiga/kwin4_effect_geometry_change) 1.5 (revision `398491e`), which animates a window that a program or a script moves or resizes | `kwin4_effect_geometry_change` | a scripted effect, `share/kwin/effects/kwin4_effect_geometry_change/` |

Each package is in the flake's `packages` for Linux and in `overlays.default`, which builds it
with the `callPackage` of the nixpkgs it is applied to, so that it follows that nixpkgs' KWin.
[`nix/tests/effects.nix`](../../nix/tests/effects.nix) has a check for each, which builds the
package and inspects its files the way KWin finds them, without a session.

At time of writing (2026-10-27):

- **KWin finds effects only when it starts.** An effect installed while KWin runs cannot be
  loaded (`isEffectSupported` is false and `loadEffect` returns false) until KWin starts
  again, which in a session means logging out and back in.
- **KWin loads every effect that is enabled by default when it starts**, with the settings
  the effect has then. KDE Rounded Corners 0.11.0 is enabled by default in its
  `metadata.json`, and at its defaults it draws a one pixel black outline inside every
  window, over its title bar. The package builds it with `"EnabledByDefault": false`: KWin
  starts without it (`loadedEffects` lists the built-in effects only), `isEffectSupported`
  is true, and `loadEffect` with `kwin4_effect_shapecorners` on `org.kde.KWin`'s `/Effects`
  loads it. The metadata is compiled into the plugin, which is where the check reads the
  flag, with `qtplugininfo`.
- **KDE Rounded Corners 0.10.0, the version in nixpkgs, draws the outline about 24 px too low
  on KWin 6.8**: across the title bar at the top, the same distance below the frame at the
  bottom, and in the right place at the sides. KWin 6.8 inverted the texture coordinates of
  the offscreen window, and Breeze's shadow is deeper below a window than above it; the error
  is the difference of the two. 0.11.0 is the release that handles it.
- **KDE Rounded Corners links `libkwin.so.6`**, so it is built against the KWin that loads
  it. With the package in a NixOS system's `environment.systemPackages`, KWin finds the
  plugin in `/run/current-system/sw/lib/qt-6/plugins/kwin/effects/plugins/`, and the plugin
  links the libkwin of the store path the running `kwin_wayland` comes from. The check
  asserts the same for the `kdePackages.kwin` of the flake's nixpkgs: the first directory of
  the plugin's `RUNPATH` that has `libkwin.so.6` is that KWin's `lib`.
- **KDE Rounded Corners draws its outer outline in the window's shadow**: its shader stops
  where the window's texture is transparent. A window without a shadow has no outer outline,
  and the outline is at most 20 px thick.
- **Geometry Change ships disabled by default**: its `metadata.json` has
  `"EnabledByDefault": false`, which the check asserts. It loads on `loadEffect`, and under
  software rendering only with `KWIN_EFFECTS_FORCE_ANIMATIONS=1` in KWin's environment (see
  [Rendering](#rendering-in-virtual-machines-and-over-rdp)): without it, a session in a
  virtual machine loads 11 effects instead of 29.
- **Geometry Change animates what a program or a script asks for**: a move and a resize made
  through KWin's scripting API are animated, and by its source an interactive move or resize
  is not. The stock effect always eases out exponentially (`QEasingCurve.OutExpo`), so the
  movement is nearly over after a third of its duration. Its settings are in the
  `[Effect-kwin4_effect_geometry_change]` group of `kwinrc` (`Duration`, `CrossFade`,
  `ExcludedWindowClasses`, and `Curve` with komorebi's patch), and it reads them again on
  `reconfigureEffect`: a `Duration` of 4000 written while it ran was the length of the next
  animation.
- **The animation moves what is painted, not the window**: the frame KWin reports for the
  window (`frameGeometry`) is the final one from the start of the animation.

### What komorebi writes for the border

KDE Rounded Corners reads its settings from the group `[Round-Corners]` of `kwinrc` (the names are
those of the effect's `options.kcfg`). komorebi sets every outline, not only the one it uses:

| Key | Value |
|---|---|
| `OuterOutlineThickness`, `InactiveOuterOutlineThickness` | the border's width, 0 to 20 |
| `OuterOutlineColor`, `InactiveOuterOutlineColor` | `r,g,b`: the focused window's border, and every other window's |
| `Size`, `InactiveCornerRadius` | the corner radius |
| `OutlineThickness`, `InactiveOutlineThickness`, `SecondOutlineThickness`, `InactiveSecondOutlineThickness` | `0`: nothing inside the window |
| `ActiveOuterOutlineAlpha`, `InactiveOuterOutlineAlpha` | `255` |
| `ActiveOuterOutlineUseCustom`, `InactiveOuterOutlineUseCustom` | `true`, so that the colors are komorebi's |
| `ActiveOuterOutlineUsePalette`, `InactiveOuterOutlineUsePalette`, `OuterOutlineIsGradient`, `InactiveOuterOutlineIsGradient` | `false` |

komorebi then calls `reconfigureEffect` on `/Effects`: at time of writing (2026-10-27), KWin 6.8.1
hands an effect the copy of `kwinrc` it read when it started, so the new values reach it only
that way, also for an effect loaded a moment before. A screenshot of a session shows the border
in the pixels just outside the frame the window has in KWin, on all four sides, in the focused
and unfocused colors, and not inside the frame.

### The patches

Each patch is a derivative work of the effect it changes, and is under the same license, the
GNU General Public License version 3; its header states the license and describes the
change. The check of a patched package asserts that the patch is in the build.

[`komorebi-border-colours.patch`](../../nix/kde-rounded-corners/komorebi-border-colours.patch),
for KDE Rounded Corners, makes two changes:

- **A color for each window.** The stock effect has one outline color for the active window
  and one for the others. The patched effect exports an object through KWin's name, which
  takes a color for each window:

  | Name | Object | Interface | Member |
  |---|---|---|---|
  | `org.kde.KWin` | `/org/komorebi/BorderColours` | `org.komorebi.BorderColours1` | `SetColours(asau)` |

  `SetColours` replaces the colors of every window at once: the windows' UUIDs as KWin writes
  them (with or without braces), and `0xRRGGBB` values in the same order. A window it does not
  name gets the colors of the settings for the active and the other windows. The object is
  registered when the effect is loaded, and removed with every color when it is unloaded.
- **A window is drawn again when its opacity changes.** The effect draws a window from a
  texture that has the window's opacity in it, and makes the texture again only when the
  contents change or the window is resized, so a change of opacity alone would not show. The
  patched effect redirects the window again when its opacity changes.

[`curve.patch`](../../nix/kwin-effect-geometry-change/curve.patch), for Geometry Change, adds
a `Curve` setting to the effect's group: the name of any of Qt's easing curves (`OutExpo`,
`InOutSine`, ...), read with the effect's other settings and used for every animation. An
unknown or missing name is the stock effect's curve, `OutExpo`. The check asserts that the
script reads `Curve` and that the effect's settings declare it.

### What komorebi writes for the animation

Geometry Change reads its settings from the group `[Effect-kwin4_effect_geometry_change]` of
`kwinrc`:

| Key | Value |
|---|---|
| `Duration` | the animation's duration, in milliseconds |
| `CrossFade` | `false`: a window that changes size is not cross-faded |
| `Curve` | the name of a Qt easing curve (`Linear`, `OutExpo`, `InOutSine`, ...): komorebi's build of the effect reads it, and the stock effect ignores it |

komorebi then calls `reconfigureEffect`, and at time of writing (2026-10-27) the effect uses the
new `Duration` for the next move: a `Duration` of 4000 written while it ran was the length of the
next animation. An animation cannot be seen in a screenshot of a short one: the screenshot takes
seconds, and the effect eases out exponentially, nearly over after a third of its duration. A
screenshot of a session a few seconds into a 30 s animation shows a window that has not got to its
place, and the same move with animation off is there at once.

## The Plasma shell

The wallpaper of a screen is the Plasma shell's, not KWin's: the shell, `plasmashell` on the
session bus as `org.kde.plasmashell`, keeps one wallpaper for each screen, not for each virtual
desktop. At time of writing (2026-10-27) komorebi asks it three things over `/PlasmaShell`, in
`org.kde.PlasmaShell`, for the output a workspace's monitor is (the name KWin gives it, `DP-1`):

1. `evaluateScript("print(screenForConnector(\"DP-1\"))")`: the number the shell gives the
   screen, which it numbers itself and names by connector; `-1` when it has none. The name goes
   into the script as a JSON string.
2. `wallpaper(screen)`: what the screen shows, as `a{sv}`, in which `wallpaperPlugin` is the
   plugin (`org.kde.image` for a picture), `Image` the picture's URL and `FillMode` a Qt
   `Image.fillMode` (2 is `PreserveAspectCrop`: the picture fills the screen and is cropped where it
   does not fit).
3. `setWallpaper("org.kde.image", {Image: URL, FillMode: 2}, screen)`: what
   `plasma-apply-wallpaperimage` calls, for one screen. It is made only when the answer to 2 is
   not already the picture, in that plugin, with that fill.

The URL is `file://` and the path, with every byte but an ASCII letter or digit, `-`, `.`, `_`, `~`
and `/` percent-encoded, so a path with a space in it works. A session with no Plasma shell (a
headless one, or one with another shell) has no such name on the bus: every call fails, and komorebi
tries again, then leaves wallpapers alone ([DESIGN.md](DESIGN.md#why-is-the-wallpaper-set-in-the-background-and-tried-again)).

## Rendering in virtual machines and over RDP

At time of writing (2026-10-27):

- A headless KWin cannot render. Mesa's software rasterizer needs `CREATE_DUMB` on the
  primary DRM node and the virtual backend offers only the render node, so KWin reports that
  compositing is active while every frame fails, and screenshots are black. KWin does not start at all with no DRM
  device. The VM nodes give the guest a `virtio-gpu` device for that reason.
- In a QEMU guest everything is drawn in software (llvmpipe on the guest's CPUs): the
  nixpkgs QEMU has only the 2D `virtio-gpu`. KWin then refuses to load animating effects
  unless `KWIN_EFFECTS_FORCE_ANIMATIONS=1` is in its environment, which the VM nodes set.
- KRDP 6.8.1, Plasma's RDP server, does not start without a TLS certificate and key, although
  its `--help` says it generates one; it logs the error and keeps running with nothing
  listening. `xrdp` is not an option, since there is no X11 session to start.
