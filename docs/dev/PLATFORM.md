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
- **Headless KWin**: `kwin_wayland --virtual`, a KWin with virtual outputs and no display,
  which the VM tests run ([vm-tests.md](vm-tests.md)).

## What komorebi needs

### KWin 6.8 or newer

KWin 6.8 is the oldest KWin komorebi supports. The bridge reads the work area of each output
for the virtual desktop that output shows (`workspace.currentDesktopForScreen`), which is part
of the per-output virtual desktops KWin 6.8 introduced. At time of writing (2026-10-27) KWin
6.8.1 is the only version komorebi has been run against; 6.7 and older have not been tried.

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
focus, is minimized or brought back, moves or changes size. The daemon waits up to 10 seconds
for the first report and does not start without it.

Every command of the window manager (move and resize, focus, raise, minimize, bring back or
close a window) is queued for the bridge, which gets the queue as the answer to its held call
and carries the commands out in KWin, in order. The script API cannot move the pointer, so
when the pointer follows the focus komorebi invokes KWin's own "Move Mouse to Focus" shortcut,
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

When it is stopped with `SIGINT`, `SIGTERM` or a `Stop` message on its socket, it asks KWin
to unload the script, closes an empty connection to each subscriber, and removes the socket.
The file `bridge.js` stays in the runtime directory, which is removed at logout. A daemon
that is killed leaves the script running in KWin, reporting to nobody, and the socket file in
the runtime directory, until the next start replaces both or the session ends.

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

## Files and directories

| Path | What | Lifetime |
|---|---|---|
| `$XDG_RUNTIME_DIR/komorebi/bridge.js` | the script KWin loads, with its configuration (the D-Bus names and the hold time of `Poll`) on its first line | written at every start; the directory is removed at logout |
| `$XDG_RUNTIME_DIR/komorebi/komorebi.sock` | the command socket | bound once the daemon is connected to KWin, after a file left there is removed; removed when the daemon stops |
| `$XDG_RUNTIME_DIR/komorebi/NAME` | the socket of a subscriber, which the subscriber binds and names in its subscription | removed by the daemon, with the subscriber, when it cannot connect to it |
| `$XDG_DATA_HOME/komorebi/komorebi.log.YYYY-MM-DD` | the daemon's log, with colors, which `komorebic log` follows | a file a day (the UTC date), never removed by komorebi |
| `$XDG_DATA_HOME/komorebi/komorebi_plaintext.log.YYYY-MM-DD` | the same log without colors | as above |

Without `XDG_RUNTIME_DIR` the bridge and the socket are in `komorebi` in the local data
directory instead, with the logs (see [`XDG_DATA_HOME`](#xdg_data_home)).

## Environment variables

### `DBUS_SESSION_BUS_ADDRESS`

The address of the session bus. If it is not set, komorebi uses `$XDG_RUNTIME_DIR/bus`, where
systemd puts the session bus, and `/run/user/<uid>/bus` when `XDG_RUNTIME_DIR` is not set
either. An address with no bus behind it is the first error under [No KWin](#no-kwin).

### `XDG_RUNTIME_DIR`

The runtime directory of the user, normally `/run/user/<uid>`: komorebi writes the bridge to
the `komorebi` directory in it, and binds its command socket there.

### `XDG_DATA_HOME`

The logs are written to `$XDG_DATA_HOME/komorebi` (`~/.local/share/komorebi` if
`XDG_DATA_HOME` is not set), which the daemon makes before anything else: a home it cannot
write to stops it at once. When `XDG_RUNTIME_DIR` is not set, the bridge and the socket go
there too.

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

## The bridge's environment

What the KWin scripting API offers a script, and what it does not, at time of writing
(2026-10-27). The bridge is written around these, and
[`kwin-api.d.ts`](../../komorebi/src/kwin/kwin-api.d.ts) declares the part of the API it
uses, which `tsc` checks it against with ECMAScript 2016's library and nothing else. Node and
TypeScript are needed only for those checks: in the session the bridge needs KWin alone.

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
cannot go where it was asked is left alone. The VM test `vm-scrolling` checks that the columns
out of view stay where komorebi put them.

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
