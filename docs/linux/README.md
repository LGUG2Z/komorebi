# komorebi for KDE

A tiling window manager for KDE Plasma on Wayland, in the family of
[komorebi](https://github.com/LGUG2Z/komorebi) for Windows and
[komorebi for Mac](https://github.com/LGUG2Z/komorebi-for-mac). The daemon
decides where each window goes, and KWin, the window manager of Plasma, does the
moving and the drawing.

## Installing it

On NixOS, the flake's module, `services.komorebi`, installs `komorebi` and
`komorebic` and starts komorebi with every Plasma session: one flake input,
one module line and `services.komorebi.enable = true`. Elsewhere, both are
built from source, or taken from a release of this repository
([Binaries](#binaries)). [`docs/linux/installation.md`](installation.md) has the
module and the build from source. The Komorebi license does not allow anyone
else to redistribute binaries.

## Running it locally

```sh
nix develop
cargo run --bin komorebi --locked -- --version
cargo run --bin komorebi --locked -- --log-level debug
cargo run --bin komorebi --locked -- --fake-backend
```

Inside a Plasma Wayland session, `komorebi` loads a script into KWin, reads the
monitors and the windows that are open, tiles them, and tiles the windows again
whenever one opens, closes, takes the focus, is minimized or comes back, until
it is stopped with Ctrl-C. It needs KWin 6.8 or newer, which the NixOS module
checks against [`nix/linux/supported-kwin.nix`](../../nix/linux/supported-kwin.nix);
[`docs/linux/dev/PLATFORM.md`](dev/PLATFORM.md) has what it expects of the
session. Outside a Plasma session it exits with an error that says what is
missing. `--fake-backend` runs the window manager against an in-memory desktop
with one monitor and no windows instead.

While it runs, the daemon takes messages on a Unix socket, `komorebi.sock` in
`$XDG_RUNTIME_DIR/komorebi` (the local data directory when there is no runtime
directory): a message can ask for the state of the window manager, ask which
monitor, workspace or container has the focus, give the focus to another
window, move the focused window to another tile, change the layout, resize a
tile, float a window, give one window the whole work area or have KWin maximize it, retile every
monitor, or stop the daemon. [`komorebi-client`](../../komorebi-client) writes them, and `komorebic`,
the command line, is how a person sends them. `komorebic start` runs the
`komorebi` on the `PATH`, and passes it what follows a `--`. In a Plasma
session:

```sh
cargo build --locked
export PATH="$PWD/target/debug:$PATH"
komorebic start
komorebic state
komorebic stop
```

`komorebic start -- --fake-backend` tries the daemon and `komorebic` without a
compositor, against the in-memory desktop.

`nix develop` provides the Rust toolchain that [`rust-toolchain.toml`](../../rust-toolchain.toml)
selects, a nightly `rustfmt`, and the tools `just check` and the flake's checks
use. With [direnv](https://direnv.net), the [`.envrc`](../../.envrc) does it on
`cd`. On Linux, `nix run .#komorebi -- --version` builds and runs the daemon
without a checkout of the toolchain.

## Trying it in a virtual machine

On a Linux machine with KVM, which includes NixOS in WSL 2 on Windows 11, one command starts a
Plasma guest in which komorebi tiles a few windows, in a window of your desktop:

```sh
nix run .#run-kde
```

`run-kde --doctor` checks the machine and says how to fix what is missing.
[`docs/linux/trying-it.md`](trying-it.md) says what you need, what you see, the key bindings to try
and what to do when it does not start.

## Binaries

The [releases](https://github.com/LGUG2Z/komorebi-for-kde/releases) of this
repository have `komorebi` and `komorebic` for x86_64 and aarch64 Linux, one zip
per architecture, with a `checksums.txt` of SHA-256 sums. The `nightly`
prerelease is built from `master` every week. CI builds the binaries;
[`CONTRIBUTING.md`](CONTRIBUTING.md#releases) says how. They use the glibc of
the system they run on, which has to be 2.39 or newer at time of writing
(2026-10-28). NixOS cannot run them without
[nix-ld](https://github.com/nix-community/nix-ld); on NixOS, build komorebi
from this flake instead.

The [Komorebi License](../../LICENSE.md) allows anyone to distribute the source code,
but not binaries built from it. The releases of this repository are the only
binaries, and there is no public binary cache for the flake: nix builds
komorebi from source.

## Crates

- [`komorebi-linux`](../../komorebi-linux): the library behind the daemon (the `komorebi` binary is the
  package [`komorebi-daemon`](../../komorebi-daemon), which calls its `daemon::run` on Linux). The library has
  the vocabulary the window manager is written in (`core`), the `Backend` trait
  through which it reaches a compositor and the in-memory desktop the tests use
  (`backend`), the backend that reaches KWin through a script it runs and edits
  the settings of KWin's it needs in the user's `kwinrc`, the parking desktop it
  hides the back of a stack and the windows behind a monocle on, and the borders
  and animations it has KWin draw (`kwin`), the border and animation settings and
  the look they make (`border_manager`, `animation`), the colors a theme gives the
  borders (`theme_manager`), the wallpapers of monitors and workspaces, which it has
  the Plasma shell show (`kwin::wallpaper`), the opacity of the windows that do not
  have the focus (`transparency_manager`),
  the model of what it tiles (`monitor`, `workspace`, `container`, `window`), the
  window manager that keeps the model (`window_manager`), loads it from the
  backend (`desktop`) and changes it when the compositor reports an event
  (`window_manager_event`, `window_manager_event_listener`, `process_event`), the
  `Ring` that holds monitors, workspaces and containers, and the lists in which
  some elements are locked in place. It answers the messages on the daemon's
  socket (`core::SocketMessage`, `process_command`), shows its state (`state`),
  keeps the monitors in step with the compositor's outputs as they change size
  and are unplugged and plugged in, and when the machine wakes or the screen is
  unlocked (`monitor_reconciliator`, `reconcile`, `kwin::power`), sends its
  subscribers a notification of each event, message and change of the monitors
  it handles (`notify_subscribers`), and is configured by a `komorebi.json` that
  it reads and reads again when it changes (`static_config`), and checks it, the
  key binding file, and the desktop it runs on, for mistakes without the daemon
  (`check`). With a key binding file, it registers keys with KWin's global
  shortcut daemon and runs their commands (`kwin::hotkeys`).
- [`komorebi-client`](../../komorebi-client): a library (shared with Windows and macOS, with a `linux` module) that sends messages to the
  daemon, reads its answers, subscribes to its notifications, and re-exports
  the types they are made of.
- [`komorebi-hotkeys`](../../komorebi-hotkeys): reads the key binding files of skhd,
  whkd and sxhkd into one model, with the keys as KDE stores them, plans each
  binding as a KDE shortcut file or as a key komorebi registers itself, and has
  the engine that decides, with no I/O of its own, which keys komorebi holds
  and what each press does. It does not depend on the daemon. Its tests read the files in
  [`komorebi-hotkeys/tests/fixtures`](../../komorebi-hotkeys/tests/fixtures), where
  each case is written in every format that can express it.
- [`komorebic`](../../komorebic): the command line (one package for the three platforms, with a `linux` module). A subcommand that asks something
  of the daemon is one message on its socket, and the others work without the daemon:
  `start` and the autostart commands start it, `check` looks at the files and the
  desktop, `quickstart` writes files, `export-kde-shortcuts` and `hotkeys compile`
  write the bindings of a key binding file that KDE can run by itself as shortcut
  files, and `configuration`, `log`, `data-directory`, `completions`, the schema
  printers and the hidden `docgen` print.
- [`komorebi-version`](../../crates/komorebi-version): the version string every binary reports.

The layouts, which turn a work area and a number of containers into a rectangle
for each, are `komorebi-layouts` from [komorebi](https://github.com/LGUG2Z/komorebi),
a git dependency at the revision `Cargo.toml` pins.

## KWin effects

The flake also builds the KWin effects komorebi drives, which are not part of
KWin:

- `kde-rounded-corners-komorebi`
  ([`nix/linux/kde-rounded-corners.nix`](../../nix/linux/kde-rounded-corners.nix)): KDE Rounded
  Corners 0.11.0, which draws an outline around each window, built so that KWin
  does not load it until it is asked to, and with a patch that lets komorebi
  give each window a color of its own.
- `kwin-effect-geometry-change-komorebi`
  ([`nix/linux/kwin-effect-geometry-change.nix`](../../nix/linux/kwin-effect-geometry-change.nix)):
  Geometry Change 1.5, a scripted effect that animates a window that a program
  or a script moves or resizes, with a patch that makes its easing curve a
  setting.

Each one is a package for Linux and an attribute of `overlays.default`, built
with the KWin of the nixpkgs the overlay is applied to.
[`nix/linux/tests/effects.nix`](../../nix/linux/tests/effects.nix) checks the files each package
installs. [`docs/linux/dev/PLATFORM.md`](dev/PLATFORM.md#kwin-effects) has what
they need from KWin.

## Checks

`just check` runs the formatting check, clippy with warnings denied, the tests,
rustdoc with warnings denied, and the type check and tests of the bridge (no
package.json: `tsc` and `node` come from the dev shell), inside `nix develop`.
The test of the signals for sleep and lock starts private D-Bus buses with
`dbus-daemon`, which the dev shell has, and does nothing where there is none.
`nix flake check` runs the same checks as derivations, plus `cargo deny`, the
TOML and Nix formatting, the tests under `cargo nextest`, and
`nixos-module-eval`, which evaluates the NixOS module in real NixOS systems,
`keybindings-fixtures`, which runs every key binding file of the fixture corpus
through `komorebic hotkeys compile` and compares what it prints with
`nix/linux/tests/keybindings.expected`, `schema-fresh`, which compares the schemas and
the options made from them with the Rust types, and `nix-options-validation`,
which checks the example configuration, in Nix and in JSON, against the schema,
all on Linux and on an Apple silicon Mac. On Linux it also builds the KWin effects and checks
their files, and boots every VM test, which needs KVM. A Mac runs the tests against the fake
desktop, since KWin only exists on Linux. `nix fmt` formats every file the checks look at.

The VM tests boot NixOS with Plasma and run a real KWin, on machines configured
with the NixOS module, so every one of them runs komorebi as the module installs
it. `just vm NAME` runs one; [`docs/linux/dev/vm-tests.md`](dev/vm-tests.md) has
the details, including how to run them on a Mac.

To try komorebi by hand in a real Plasma session, the flake has Plasma guests
to develop in. For an Apple silicon Mac there are two that QEMU runs, one with a
session that can be reached over RDP, and a disk image for UTM, which renders on
the Mac's GPU. For a Linux machine with KVM there are the same two QEMU guests,
for `x86_64` and for `aarch64`, and a demo guest of the session guest, which
shows komorebi tiling a few windows in the first minute. On Linux,
`nix run .#run-kde` (or `just run-kde`) starts the demo in a window, and
`run-kde --doctor` says what the machine lacks.
[`docs/linux/dev/utm.md`](dev/utm.md) says how to build and run them;
`plasma-guests-eval` evaluates them on every system, and `run-kde-dry-run` and
`run-kde-doctor` run the command against machines that a test makes up.

## How it fits together

The daemon decides where every window goes and asks the compositor to put it
there. Everything it needs from the compositor is behind one trait, `Backend`,
so the window manager logic runs against a real compositor and against an
in-memory fake desktop alike. [`docs/linux/dev/ARCHITECTURE.md`](dev/ARCHITECTURE.md)
describes the trait and where each of its rules is enforced.

On KDE Plasma the backend is in two parts. A script that komorebi loads into
KWin, the bridge, reports what KWin sees over the session bus, and carries out
the commands it is given as the answer to a call it holds open. The daemon keeps
a table of the windows and monitors from those reports, answers the window
manager's questions from it, and turns each report into an event for the window
manager. If KWin restarts, the daemon loads the bridge into the new one.

komorebi's workspaces are KWin's own virtual desktops: workspace `i` is desktop
`i`, and each monitor shows a desktop of its own, which the daemon has KWin do by
turning on its per-output virtual desktops (the setting is put back when the
daemon stops). Showing a workspace is a request to KWin, which shows the
windows of that desktop, hides the others, and gives the focus to one of them.
When something else switches a monitor to another desktop (a pager, a shortcut,
an application raising a window of another desktop), or puts a window on
another desktop, the window manager follows.

When a monitor changes size (a resolution, a scale, a panel that takes more room),
the bridge reports the outputs again and the monitor reconciliator, a thread of
the daemon, reads the size and the work area of every monitor and tiles the
workspace in front of the focused monitor in the area it now has. When an output
is unplugged, KWin takes its windows to the outputs that are left, and the
daemon takes the monitor's containers over to the monitors the windows are on,
tiles them there, and keeps the monitor, with its settings, for when it is
plugged in again. A window that something else moves to another output (a
shortcut, an application, KWin itself) is followed to the workspace of the same
number on that monitor and tiled there.

While the screen is locked or the machine is asleep KWin sends its clients no
frame callbacks, so what the daemon asked for in that time is not done. The
daemon watches for the machine waking up (logind's `PrepareForSleep`) and for
the lock screen going away (`org.freedesktop.ScreenSaver`), and then looks at
the monitors and at where every window is, and puts each in its tile. The lock
screen is a window of KWin's that the bridge does not report as an application
window, so it has no tile.

The window manager models the desktop as monitors, each with a ring of
workspaces, each with a ring of containers, each with a ring of windows. A
workspace's layout gives each of its containers a tile of the monitor's work
area, and every window of a container is moved onto its tile. At startup the
window manager loads the monitors and the windows that are open from the
backend and tiles them; after that it changes the model when the compositor
reports that a window was shown, closed, focused, minimized or restored, and
tiles the workspace in front again. A window the user drags with the pointer
onto the tile of another swaps places with it, and dragging the edge of a tile
resizes it, as `resize-edge` does, with the tiles next to the edge following it
while it is dragged. A window open at startup goes to the workspace whose desktop
it is on.

A container with more than one window is a stack: the layout gives it one tile,
and the window in front is in sight. The others are hidden, which on KWin means
moved to one more virtual desktop, `komorebi-park`, that nobody looks at and that
is not a workspace. The daemon makes it when it first needs it, removes it when
it stops, and removes one that a run that did not stop left behind. A monocle
hides the windows of the other containers there too.

Everything else reaches the daemon as a message on its socket: one JSON
`SocketMessage` per line, handled one at a time with the window manager
locked. A message that asks a question is answered on the same connection. A
message that gives an order changes the model the way an event does: `focus`
and `cycle-focus` give the focus to the container on one side of the focused
one in the layout, or to the next one in the workspace's order, and `move` and
`cycle-move` swap the focused container with that one and tile the workspace
again. `change-layout`, `cycle-layout` and `flip-layout` tile the workspace in
another layout, `layout-ratios` and `scrolling-layout-columns` tune it,
`toggle-tiling` stops it moving windows and starts it again,
`preselect-direction` reserves a tile for the next window, `resize-edge` and
`resize-axis` move the edges of the focused tile, `promote` brings the
focused window to the largest tile, `toggle-float` takes it out of the
layout, `toggle-monocle` gives its container the whole work area, hiding the
others, and puts it back, `toggle-maximize` has KWin maximize the focused window
and restore it (a window the user maximizes leaves the layout the same way, and
a fullscreen window is left where it is),
`toggle-workspace-layer` switches the focus commands between the tiles and the
floating windows, and `toggle-lock` keeps its container at its
place. The scrolling layout is a strip of columns that runs past the edges of
the monitor and scrolls to the window that has the focus. `minimize` and `close`
ask KWin to minimize or close the focused window, `unmanage` takes it out of the
layout until `manage` takes it back, and `toggle-pause` stops the daemon acting
on events and orders until it is resumed. The padding commands set the gaps around the tiles, and
the work area offsets keep parts of a monitor free of tiles. Rules decide which
windows are managed at all: `ignore-rule` keeps an application out of the
layout, `manage-rule` tiles it whatever the ignore rules say, and
`session-float-rule` floats the focused window and the windows like it, and a
workspace rule (`workspace-rule`, `initial-workspace-rule`, their named forms,
or `workspace_rules` in `komorebi.json`) sends the windows of an application to a
workspace, once or whenever the window manager looks, which it does at every event.
`focus-workspace` shows another workspace, and `move-to-workspace` and
`send-to-workspace` put the focused window on another workspace, with and
without showing it; their `cycle-` and `-last-workspace` forms go to the next,
the previous or the last workspace, `workspace-name` names a workspace for
`focus-named-workspace` and the other named commands, and `new-workspace` and
`close-workspace` add and remove one.
At the edge of a monitor, `focus` and `move` cross to the monitor on that side;
`focus-monitor`, `move-to-monitor`, `send-to-monitor` and their kind work on the
monitors, and `eager-focus` finds the window of an application wherever it is.
`stack` puts the focused window and the one next to it in one stack, `unstack`
takes it out again, and `stack-all` and `unstack-all` do it for a whole
workspace; `cycle-stack` and `focus-stack-window` bring another window of a
stack to the front, and `cycle-stack-index` moves the front one along it.
`toggle-window-container-behaviour` makes a window that opens join
the focused stack, and `toggle-float-override` makes it float.

The daemon is configured by `komorebi.json`: the file `--config` names, or the
one in `~/.config/komorebi`. It reads the file when it starts, once it has the
monitors, and gives each monitor the workspaces the file lists for it, with their
names, layouts, paddings and rules. It watches the file, and when it changes
applies it again to the windows it has. It also sets what a window that opens
gets (`window_container_behaviour`, `float_override`, `floating_layer_behaviour`,
for a workspace or for all of them) and what focusing does in monocle
(`monocle_focus_behaviour`). Where a new window goes in the layout is the
workspace's `initial_window_placement_rules`: after the focused container unless
they say another place, which can depend on the window's program, app id or
title. A key that komorebi for KDE does not read
is ignored, so one file serves every platform. Rules for particular applications
can be in files of their own, which `app_specific_configuration_path` names.
`workspace-layout-rule` makes a
workspace change layout with the number of windows it has, as the file's
`layout_rules` do, and `generate-static-config` prints the settings the daemon
has as a `komorebi.json`, and `replace-configuration` makes the daemon start over from
another file. `quickstart` writes an example configuration and sample key bindings, and
`enable-autostart` an autostart entry that has Plasma start komorebi at login. The Rust types that read the file describe it as a JSON Schema
(`schemas/linux/schema.json`, written by `just jsonschema` and printed by `static-config-schema`), and the
module's `config` option is typed from that schema (`nix/linux/komorebi-options.nix`, written by
`just nixgen`), so a configuration written in Nix is checked when it is evaluated.
`check` reads the file the way the daemon does and says in words what is wrong with it: JSON that
does not parse, with the line, a value komorebi does not read, a key that is not a setting (with
the nearest one that is), a workspace that is not tiled, and an applications file that is not
there. It looks at the key binding file as well (how many bindings it has, the lines that cannot
be read, and whether the shortcut files `hotkeys compile` wrote are the ones it would write now),
and at the desktop (a Plasma session on Wayland, KWin 6 on the session bus, the daemon and its
script in KWin, and what starts komorebi at login), unless `--configuration-only` leaves that out.
It exits with status 1 if something is an error.

A program such as a status bar can subscribe to the daemon's notifications: it
listens on a socket of its own, and after each event, each message and each
change of the monitors, the daemon connects to it and writes it with the state
of the window manager.

On NixOS, the module [`nix/linux/nixos-module.nix`](../../nix/linux/nixos-module.nix) runs the
daemon as a systemd user unit that the Plasma session starts, and systemd
starts it again if it fails.

The borders are KWin's: komorebi asks the backend for a look, a border of a
width, a radius and two colors, and the KWin backend has the KDE Rounded Corners
effect draw it, which the module installs. A border takes room around each
window, so the layout leaves the room while the compositor says it draws one, and
komorebi gives each window the border color of what it is to the layout: the focused
window, the others, a locked container, a floating window. `border`, `border-colour`,
`border-width` and `border-offset` set it. Windows going to their places are
animated by another effect, Geometry Change, which `animation`,
`animation-duration` and `animation-style` turn on and set. `transparency` makes
the windows that do not have the focus see-through, by the opacity KWin gives a
window, and `focus-follows-mouse` has KWin give the focus to the window under the
pointer, by its own focus policy. A theme gives the borders their colors, and
the palette of a picture is made and kept in the cache directory. A monitor or a workspace
can have a wallpaper, which the Plasma shell is asked to show when the workspace comes into view,
and which gives the borders the colors of its palette.

Key bindings are written in the format of skhd, whkd or sxhkd. `komorebi-hotkeys`
reads such a file into one model and sorts its bindings into static shortcuts,
which KDE runs by itself from a `.desktop` file, and those that need komorebi
to register the keys and decide what each press does. `komorebic hotkeys
compile` writes the static shortcuts, and the daemon runs the engine for the
rest: it registers their keys with KWin's global shortcut daemon over the
session bus and runs the command of each press. On NixOS, the module's
`keybindings` options do both: the static shortcuts go into the system profile,
and the unit starts the daemon with the file.

## Documentation

The user guide starts at [`docs/linux/index.md`](index.md),
[`docs/linux/trying-it.md`](trying-it.md) starts a guest in which komorebi tiles a few windows,
[`docs/linux/installation.md`](installation.md) says how to install komorebi,
[`docs/linux/usage/configuration.md`](usage/configuration.md) explains `komorebi.json`,
[`docs/linux/example-configurations.md`](example-configurations.md) has an example of it,
[`docs/linux/usage/keybindings.md`](usage/keybindings.md) how to give komorebi key bindings,
and [`docs/linux/cli`](cli) has a page for every `komorebic` subcommand, which
`just docgen` writes from the subcommands' help.
`cargo doc --workspace --no-deps --open` builds the API documentation.
[`CONTRIBUTING.md`](CONTRIBUTING.md) says how to work on the code.
[`docs/linux/dev/ARCHITECTURE.md`](dev/ARCHITECTURE.md) describes the model,
[`docs/linux/dev/DESIGN.md`](dev/DESIGN.md) says why it is built that way,
[`docs/linux/dev/PLATFORM.md`](dev/PLATFORM.md) what it needs from KDE Plasma,
[`docs/linux/dev/COMPARE.md`](dev/COMPARE.md) what is the same and what is different in
komorebi for Windows and komorebi for Mac,
[`docs/linux/dev/vm-tests.md`](dev/vm-tests.md) how the VM tests work, and
[`docs/linux/dev/utm.md`](dev/utm.md) how to develop in a Plasma guest, and
[`docs/linux/dev/wsl2-probe.md`](dev/wsl2-probe.md) how to find out whether a
NixOS-WSL2 machine can run one (`nix run .#wsl2-probe`).

## License

komorebi for KDE is under the [Komorebi License](../../LICENSE.md). Contributions
are accepted under the terms in [`CONTRIBUTING.md`](CONTRIBUTING.md).

The license lets others distribute the source code but not binaries, so there
is no public binary cache for what the flake builds: komorebi and the KWin
effects are built on each machine that installs them, with their dependencies
from nixpkgs. The patches to the effects in [`nix`](../../nix/linux) are derivative works
of the effects, which are under the GNU General Public License version 3, and
are under the same license, as the header of each one states.
