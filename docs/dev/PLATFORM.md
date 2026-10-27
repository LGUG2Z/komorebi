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

#### What komorebi does

It connects to the session bus as `org.komorebi.Bridge`, writes the bridge to
`$XDG_RUNTIME_DIR/komorebi/bridge.js`, and asks KWin to load it under the name
`komorebi-bridge`, replacing a script of that name left by an earlier run. The script reports
the windows and outputs it finds, and from then on every window that opens, closes, takes the
focus, is minimized or brought back, moves or changes size. The daemon waits up to 10 seconds
for the first report and does not start without it.

The KWin backend only reads the desktop. Every command the window manager gives it (move,
focus, raise, minimize, close a window, move the pointer) fails with "the KWin backend only
reads the desktop", which the window manager logs.

#### What komorebi leaves behind

When it is stopped with `SIGINT` or `SIGTERM` it asks KWin to unload the script. The file
`bridge.js` stays in the runtime directory, which is removed at logout. A daemon that is
killed leaves the script running in KWin, reporting to nobody, until the next start replaces
it or the session ends.

### A headless KWin

`kwin_wayland --virtual` behaves like the KWin of a session for everything komorebi does:
the VM tests run the daemon against one, on a private session bus of its own. It cannot draw
(see [Rendering](#rendering-in-virtual-machines-and-over-rdp)), so nothing that depends on
pixels can be tested against it.

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
| `org.kde.KWin` | `/Scripting` | `org.kde.kwin.Scripting` | `unloadScript(s)`, `loadScript(ss) -> i` | at start; `unloadScript` again at exit |
| `org.kde.KWin` | `/Scripting/Script<id>` | `org.kde.kwin.Script` | `run()` | at start |

## Files and directories

| Path | What | Lifetime |
|---|---|---|
| `$XDG_RUNTIME_DIR/komorebi/bridge.js` | the script KWin loads, with its configuration (the D-Bus names and the hold time of `Poll`) on its first line | written at every start; the directory is removed at logout |

Without `XDG_RUNTIME_DIR` the directory is `komorebi` in the local data directory instead
(see [`XDG_DATA_HOME`](#xdg_data_home)).

## Environment variables

### `DBUS_SESSION_BUS_ADDRESS`

The address of the session bus. If it is not set, komorebi uses `$XDG_RUNTIME_DIR/bus`, where
systemd puts the session bus, and `/run/user/<uid>/bus` when `XDG_RUNTIME_DIR` is not set
either. An address with no bus behind it is the first error under [No KWin](#no-kwin).

### `XDG_RUNTIME_DIR`

The runtime directory of the user, normally `/run/user/<uid>`: komorebi writes the bridge to
the `komorebi` directory in it.

### `XDG_DATA_HOME`

Read only when `XDG_RUNTIME_DIR` is not set: the bridge is then written to
`$XDG_DATA_HOME/komorebi` (`~/.local/share/komorebi` if `XDG_DATA_HOME` is not set either).

### `RUST_LOG`

Which log lines the daemon prints, in the syntax of `tracing-subscriber`'s `EnvFilter`
(`komorebi=debug`, `komorebi::kwin=trace`). If it is not set, `--log-level` sets it to
`komorebi=<level>`; `--log-level` defaults to `info`.

### `RUST_LIB_BACKTRACE`

Whether an error carries a backtrace. komorebi sets it to `1` when it is not set.

### `NO_COLOR`

When set to anything but the empty string, the log has no ANSI colors, which is what a log
written to a file wants. The VM tests set it.

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
