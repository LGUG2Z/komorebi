# VM tests

The unit tests run against a fake desktop. They cannot say whether KWin does
what the daemon asks of it, so a second kind of test boots a NixOS virtual
machine with Plasma in it, starts a real KWin, and drives it. This page says how
they are laid out, how to run them, and how to add one.

## What runs where

| Where | How | Accelerator |
|---|---|---|
| CI, `x86_64-linux` | `nix build -L .#checks.x86_64-linux.vm-NAME` | KVM |
| CI, `x86_64-linux`, the weekly canary | the same, with nixpkgs moved to the tip of nixpkgs-unstable | KVM |
| A Linux machine with KVM | `just vm NAME` | KVM |
| A Mac | `just vm NAME`, which runs `scripts/vm-mac` | HVF |
| A Linux machine with no KVM | `nix build -L .#legacyPackages.SYSTEM.vmTcg.NAME` | TCG |

CI runs the `fast` tier on every pull request and the `slow` tier on `master`,
every week, and on pull requests labeled `ci:vm-slow`
([`ci.yaml`](../../.github/workflows/ci.yaml)). The slow jobs run the driver
by hand instead of building the check, so that the screenshots and journals of a
failed run can be uploaded as an artifact. KVM on GitHub-hosted runners is
documented for public repositories; the first step of each VM job checks that
`/dev/kvm` is writable and fails there if it is not.

The `canary` job runs both tiers once more every week, against the tip of
nixpkgs-unstable instead of the nixpkgs `flake.lock` pins: it runs
`nix flake update nixpkgs` in a copy of the checkout, builds every check there,
the suites one after the other on one machine, and opens an issue when one
fails ([`CONTRIBUTING.md`](../../CONTRIBUTING.md#continuous-integration)). It is
how a new KWin or Plasma that breaks a suite is found before the lock moves to
it.

TCG is software emulation. It is slow enough that only short suites are worth
running that way, and the library's `scale` argument (below) is 8 for it.

Whether a machine has working KVM is not always obvious, and under WSL2 it
depends on settings of Windows. `nix run .#wsl2-probe` boots a tiny guest with
and without KVM and times both ([wsl2-probe.md](wsl2-probe.md)).

The suites are what `testedMax` in
[`nix/supported-kwin.nix`](../../nix/supported-kwin.nix) means: the newest KWin
every suite has passed against. When the weekly canary job, which runs every
suite against the tip of nixpkgs-unstable, passes with a newer KWin, a commit
raises `testedMax` to it and says which suites ran
([CONTRIBUTING.md](../../CONTRIBUTING.md#the-nixos-module)).

Each suite is a directory under
[`nix/tests/vm/suites`](../../nix/tests/vm/suites). The generator in
[`nix/tests/vm/default.nix`](../../nix/tests/vm/default.nix) lists that
directory, so a new suite is a new directory and nothing else: it appears as
`checks.<system>.vm-NAME` on Linux and as `legacyPackages.<system>.vm.NAME`
everywhere. `legacyPackages.<system>.vmTiers` lists the names of each tier as
JSON, which is what CI reads.

A suite has two files:

- `default.nix` sets `tier` (`"fast"` or `"slow"`), `kind` (the node it runs on)
  and `timeoutMinutes`, and may set `node`, a NixOS module that adds to the node
  for this suite alone (more memory, the clients it starts, the options of
  `services.komorebi`).
- `test.py` is the test, a Python script for the NixOS test driver. It is
  prefixed with `SCALE = <number>`, how many times slower than KVM the machine
  is. Suites that run the same test on different nodes set `script` to a file
  they share instead, relative to their directory: `vm-keys-skhd`,
  `vm-keys-whkd` and `vm-keys-sxhkd` run `../../keys-corpus.py`, each with the
  key bindings of its format in its `node`.

## Nodes

`kind` picks a file under
[`nix/tests/vm/nodes`](../../nix/tests/vm/nodes).

Both nodes are built with the NixOS module, as a user's machine is: they import
`nixosModules.default`, which the generator passes to them, and enable
`services.komorebi`, which installs `komorebi` and `komorebic`, built from the
commit under test. `services.komorebi.logLevel` is `"debug"`, because the tests
read the daemon's debug log. A suite can change either option in its `node`:
`latency`, which times the daemon, runs it at `"info"`.
The checks of the module that need no machine are `nixos-module-eval`
([`nix/tests/nixos-module.nix`](../../nix/tests/nixos-module.nix)), and the
suite `module` checks its unit in a Plasma session.

`headless` is a machine with Plasma installed and no login. Each KWin a test
starts is a pair of user units of the test user, `session-bus@NAME` (a private
D-Bus session bus) and `kwin-virtual@NAME` (`kwin_wayland --virtual` with its
own Wayland socket), so a test can run several sessions side by side and none
touches another. A virtual KWin has no real output, so it cannot draw: it can
place windows and answer questions about them, but it cannot take a screenshot.
A third unit, `komorebi@NAME`, runs the daemon in a session: it is the module's
unit, `komorebi.service`, moved to the session's bus and Wayland socket, and runs
the module's `execStart` followed by the arguments the test gives. It is not
bound to KWin's unit, so a test can restart KWin under a running daemon. The
module's unit itself starts with a Plasma session, which this node does not
have.

KWin wants a DRM render node, which needs a GPU. The test driver gives every aarch64 guest one
`virtio-gpu-pci`; on x86_64 the node adds it, unless `komorebi.vm.renderNode` is false.

`session` is the headless node plus a login: plasma-login-manager logs the test
user in by itself, as in nixpkgs' own `plasma6` test, and Plasma draws on the
machine's display device. The login starts the module's unit, so komorebi runs
in the session from the start, as on a user's machine; `render-smoke`, which
checks that the session draws, turns it off. It is the node for anything that
needs pixels: `features` reads what komorebi does from screenshots of it. The
choice of login manager is made in
[`nodes/session.nix`](../../nix/tests/vm/nodes/session.nix) and nowhere else;
the file says how to switch to sddm, the other one that has been seen to work.
Its display is 1280x800, because screenshots are taken of the machine's one
display device: the `virtio-gpu-pci` that the test driver adds to every aarch64
guest, and the default VGA of an x86_64 one. The node sets
`komorebi.vm.renderNode` to false, so that an x86_64 guest does not get a second
device: KWin would draw on it, and the screenshot would show the other one.
The node also has uinput, with the test user in the group `uinput`, and the
program `komorebi-pointer`, which makes a pointer device with it: a test drives
the pointer from inside the guest, and KWin takes it as a tablet.

On aarch64 under HVF, at time of writing (2026-10-27), the session has the
Wayland socket after 11 to 19 seconds and a painted desktop after 35 seconds.

A suite's `node` can import a module of the repository: `rdp` imports
[`nix/dev/krdp.nix`](../../nix/dev/krdp.nix), the RDP server of the Plasma
guests to develop in, so that the suite checks the unit those guests run, and
`run-kde` imports [`nix/dev/run-kde.nix`](../../nix/dev/run-kde.nix), the demo
guest that the command `run-kde` starts, so that what CI boots is what the
command starts. The node of `run-kde` also makes the directory that the host
shares with that guest (`/run/run-kde`), owned by a user the guest does not
have, with a password and a public key in it, as the guest finds it once it has
mounted it. The guests themselves, which stay up for a person to use, are in
[utm.md](utm.md).

## The library

Suites import from `komorebi_vm`, in
[`nix/tests/vm/lib`](../../nix/tests/vm/lib). It does not depend on the test
driver. It depends on the small `Machine` protocol, and its unit tests pass a
fake machine, so the `vm-lib` check tests it without booting anything. The
check also runs `ruff` and `ty` over it. The driver type checks every
`test.py` with `ty` and `ruff` too, so a typo in a suite fails the build, not
the run.

- The package `komorebi_vm.core` is what does not depend on the platform: `Checks`,
  `Rect` and the pixel helpers. It imports nothing of KDE, so that it can move to
  the Windows port unchanged. `komorebi_vm` re-exports them, so that a suite
  imports them from the same place as before.
- `Snapshot` (`core/snapshot.py`) is the daemon's state in one shape for every port:
  `snapshot.parse(json)` reads the `komorebic state` of kfde and of kfw into
  monitors, workspaces, containers and windows. What only one port's state has
  goes in `Snapshot.extra`. Its unit tests read states recorded from a real run of each
  port (`tests/fixtures`: kfde's from `core-e2e`, which writes them into its output, and
  kfw's from a Windows guest), and a scene written from the serializers for the shapes
  that no run has recorded.
- `Platform` (`core/platform.py`) is what a scenario needs of a platform: start the
  daemon, run `komorebic`, read the `Snapshot`, open a window, ask whether the
  daemon put it away (`hidden`), ask the operating system for its frame
  (`os_window`, the oracle: the daemon's state is the thing under test), and take
  a screenshot. Where the platform has a pointer (`has("pointer")`), a scenario moves
  it with `pointer.drag`; where it has none, the scenario says so and is skipped.
  `core/scenarios` has the scenarios written against it: `tiling`, `focus`, `moving`,
  `stacks`, `monocle`, `workspaces`, `closing`, `resize`, `rules`, `hotplug`, `commands`
  (every layout and about 216 commands), `lifecycle` (stop, start again), `placement`,
  `startup_rules` and `focus_follows_mouse`. A method is added with the first scenario that
  needs it: `restart_with(config)` starts the daemon afresh with a `komorebi.json` the
  scenario gives. `KdePlatform` (`kde_platform.py`) is a `Kde` session as a `Platform`.
  [Writing a scenario](#writing-a-scenario) says how to add one.

  A check that a port is known to fail is recorded with `Checks.known_failure(description,
  condition, reason)`: it does not fail the suite, the totals say `known failures: N`, and
  one that holds fails the suite, so that the marker goes with the bug. Scenarios that can
  hit one take the reason from the runner (`unknown_monitor`, `placement`).

  The scenarios run on two platforms. KDE runs them in `core-e2e`, except `resize` and
  `focus_follows_mouse`, which need a pointer and run at the end of `drag-resize`. A
  Windows 11 guest under QEMU, which kfw drives from its own tree with this `core/` copied
  unchanged (`scripts/sync-e2e-core` keeps the copy in step until `core/` moves there),
  runs all of them against kfw's daemon. [BEHAVIOUR.md](BEHAVIOUR.md) says what each
  scenario found on each port, and what turned red when the daemon was deliberately broken.
- `Checks` counts checks and fails once, at the end, so that one failure does
  not hide the ones after it. `finish()` prints `passed: N failed: M`, and its
  error names the checks that failed, which `nix build` shows in the last lines
  of the run.
- `Kde` is one KWin session. `Kde.headless(machine, name, size=, outputs=,
  scale=)` starts the units and waits for KWin to own its name on the private
  bus. `wid`, `rect`, `title` and `active` read windows through `kdotool`,
  `outputs()` counts the enabled outputs through `kscreen-doctor`, `spawn` starts
  a client, and `wait_for(predicate, timeout)` polls, with the timeout
  multiplied by `scale`.
- `Kde.outputs` has the outputs: `names()` lists them from left to right, and
  `disable(name)`, `enable(name)` and `scale(name, factor)` change them with
  `kscreen-doctor`, which is how a suite changes the monitors under the daemon.
  A disabled output keeps its name, so `names(enabled=False)` finds it to enable
  again.
- `Kde.session(machine, scale=)` attaches to the session node's own Plasma
  session once its desktop is up, and `shot(name)` returns a screenshot as a
  Pillow image, keeping `NAME.png` in the output directory.
- `activate`, `minimize`, `maximize`, `unmaximize`, `fullscreen` and `close` act on a window
  through `kdotool`, as the user would (`maximize` does what the title bar's button does, and the
  daemon is not asked), and `wid_by_class` finds a window by its app id or X11 class.
  `Kde.headless(..., xwayland=True)` gives the session XWayland, for X11
  clients.
- `start_komorebi(*args)` starts the daemon in the session, with the module's
  `execStart` and `args` after it (not `--log-level`, which `execStart` has),
  `stop_komorebi()` stops it with `SIGTERM`, `kill_komorebi()` kills it with
  `SIGKILL`, as a crash does, `komorebi_log()` returns its log since it was
  last started, and `script_loaded()` asks KWin whether it runs the bridge.
  `restart_kwin()` stops the session's KWin and starts a new one on the same
  bus, as when the compositor of a running session crashes, and waits for it.
- `komorebic(*args)` runs `komorebic` in the session and returns what it
  printed, failing the test if it fails. `state()` is the daemon's state as
  `komorebic state` prints it, `managed(workspace)` the number of windows the
  daemon has on a workspace of the first monitor, `managed_all()` the number it
  has on every workspace of every monitor (floating windows, monocle containers
  and maximized windows too), `containers(workspace)` the titles of the windows
  of each of its containers, `monocle(workspace)` those of its monocle
  container, and `focused_title(workspace)` the title of the window it has
  focused there.
- `daemon` reads the daemon's view of the desktop from that log, at the debug
  level the nodes set: `daemon.events(log)` lists the window events it handled,
  in order, `daemon.window_ids(log)` maps a window's title to the daemon's id
  for it, and `daemon.requests(log)` returns the frame the daemon last asked KWin
  for, for each window.
- `desk(wid)` is the number of the desktop a window is on and `shown()` the one
  KWin shows, both counted from 1 as `kdotool` counts, `desktops()` how many
  desktops KWin has, `desktop_names()` their names in order (the parking desktop
  of a stack is the one named `komorebi-park`), `kwinrc(group, key)` a value of
  the user's `kwinrc`, and `kwinrc_group(group)` every key of a group, which a
  test compares before and after komorebi to see that the user's settings are
  put back.
- `effect_supported(name)` and `effect_loaded(name)` ask KWin whether an effect is
  installed (KWin found it when it started) and loaded, and `unload_effect(name)`
  unloads one, whoever loaded it.
- `colored_window(title, color)` opens a terminal that is one color all over, and
  waits for KWin to have it, and `close_colored_windows()` closes them: a test
  reads the color of a pixel of a screenshot, and wants nothing else there.
- `pixels` reads a screenshot: `pixels.pixel(image, x, y)` is the color of a
  pixel, `pixels.near(a, b)` compares two colors within a tolerance, since a
  compositor blends the edge of what it draws, `pixels.ring(image, frame,
  color)` names the sides of a window's frame that have no ring of `color` just
  outside them, which is how a test sees a border, and `pixels.leftmost(image,
  y, color)` finds where a row first has a color, which is how a test sees how
  far along a window is on its way.
- `kwin_script(name, source, keep=)` runs a KWin script and returns the lines it
  reported: the script calls `report(...)`, which `print`s a line marked with
  `name` to KWin's log, and ends with `report("done")`. A script that is kept
  goes on reporting what its signal handlers see, and `kwin_reports(name)`
  reads every line it has reported so far from the journal.
- `monitor_bridge(path)` records the bridge's traffic on the session bus with
  `dbus-monitor`, and `latency.rounds` times each batch of commands in it, from
  the daemon's answer to the last frame KWin reports for the windows it moved.
- `Pointer.start(machine, (width, height), scale=)` starts `komorebi-pointer` on
  a session node, for a screen of that size, and `move(x, y)`, `down()`, `up()`,
  `glide(x, y, steps, ms)` and `drag(start, end)` move and click it. Each
  returns once the program has done it, and a glide's moves are made in the
  guest, milliseconds apart: moves that each wait for the test are seconds
  apart, and KWin was seen to lose the horizontal part of such a drag. The
  pointer lands within a pixel of where it is asked. The program itself is
  [`komorebi_vm/uinput.py`](../../nix/tests/vm/lib/komorebi_vm/uinput.py),
  whose arithmetic the `vm-lib` check tests.
- `RdpClient(machine, kde)` is FreeRDP's client on an X server of its own, for a
  node that runs KRDP: `listening()` asks the guest whether the server has its
  port, `logs_in(password)` whether a client with that password is let in,
  `connect(password)` starts a client that stays, `picture()` reads what it
  shows from its X server, and `shows_display()` waits for that to be the
  display (more than a thousand colors, within 16 of 255 on average of a
  screenshot). The suites `rdp` and `run-kde` both use it.
- `Keys(machine).send("alt-shift-k")` presses a combo on the machine's
  keyboard, written as KDE writes it in lower case with `-` between the keys
  (`meta` and `super` are the left Super key).
- `spawn_komorebi(*args)` starts a daemon of the test's own in the session
  node's Plasma session, logging to the file `komorebi_log()` reads;
  `kill_spawned_komorebi()` kills it with `SIGKILL`; `shortcut_actions()` lists the
  actions komorebi has registered with kglobalaccel. A suite that spawns its own
  daemon turns the module's unit off in its `node`.

## Pressing keys

A key test presses keys with QEMU's `sendkey` (`machine.send_key`), through
`Keys`. The key reaches the guest's kernel as a key of the machine's virtual
keyboard, and KWin sees it as it sees a person's: a global shortcut fires on
it, and any other key goes to the focused window. QEMU holds the keys for 100
ms, less than the delay before a key repeats. Phase 0 drove keys from a uinput
device inside the guest instead, as the pointer is driven now; `sendkey` needs
no helper program for keys, and on x86_64 under KVM `vm-keys-static` passed with
it on its first run (2026-10-27).

Global shortcuts need the session node: the headless KWin runs with
`--no-global-shortcuts`, so it has no `org.kde.kglobalaccel`. A static
shortcut written into the system profile by the suite's `node` (as
`vm-keys-static` does) is found when the session starts.

Poll for what the test needs with `wait_for`. A fixed sleep is too long on
KVM and too short on TCG.

Two traps are worth knowing about before writing a screenshot test:

- A splash screen is not a blank screen. KSplash has more than a thousand colors
  on it, so "the screenshot is not blank" passes while Plasma is still
  starting. `Kde.session` waits for the splash process to exit, and a test
  should check a pixel it knows, such as the middle of a terminal.
- The panel comes after the shell. A window tiled before the panel was made is
  tiled under it, which hides the bottom of its border from a screenshot, and
  nothing tiles it again. `Kde.session` waits until the shell has made a panel. KWin takes
  the room for the panel a while after that, and a daemon learns of it up to a second later: a
  test that opens windows right after the daemon starts waits with
  `Kde.wait_for_panel_room` first. It is a wait and not a check, because a session that is too
  slow may never take the room.
- NixOS wraps programs, so a process's name is the wrapper's (`.plasmashell-wrapped`),
  and `pgrep -x plasmashell` never matches. Match the command line instead, with
  a bracket in the pattern (`pgrep -f '[p]lasmashell'`) so that the shell running
  `pgrep` does not match itself.

## Running a test on a Mac

QEMU has to run on the Mac to use HVF, and the guest is Linux, so the guest's
closure is built on a Linux builder and the driver runs on the Mac. The nix
sandbox on macOS cannot give QEMU the hypervisor, so `scripts/vm-mac` runs the
driver directly instead of building the check.

```sh
export KOMOREBI_BUILDER=ssh-ng://builder@localhost
export NIX_SSHOPTS="-p 31022 -i $HOME/.ssh/builder_ed25519 -o IdentitiesOnly=yes"
just vm kwin
```

`KOMOREBI_BUILDER` is the store URL of an `aarch64-linux` builder you can
reach. `nix run nixpkgs#darwin.linux-builder` starts one, with one core and 3 GB
of memory by default; a Plasma guest needs more than that (6 cores and 10 GB
worked, at time of writing, 2026-10-27), and the builder's 20 GB disk holds one
Plasma system closure of about 10 GB, so run `nix-collect-garbage` on it
between builds of different guests.

The script prints where the artifacts (screenshots, logs) are, under
`.vm/out/NAME`.

## Writing a test

1. Create `nix/tests/vm/suites/NAME/default.nix` and `test.py`. Start from
   [`suites/kwin`](../../nix/tests/vm/suites/kwin), or from
   [`suites/kwin-events`](../../nix/tests/vm/suites/kwin-events) for a test that
   runs the daemon.
2. Pick the tier. `fast` suites are the pull request gate and should take
   minutes. `slow` suites run on `master` and every week.
3. Run it: `just vm NAME`.
4. If the test covers behavior the daemon has, write the Rust test that covers
   the same behavior against the fake desktop in the same commit, and say in
   the VM test's first comment what only a real KWin can tell.

Add `git add -N` for new files first: a flake only sees files git knows about.

## Writing a scenario

A scenario is a function `run(platform, checks, ...)` in
`nix/tests/vm/lib/komorebi_vm/core/scenarios/`. It is what runs on every
platform, so it follows four rules:

- It reaches the desktop only through the `Platform`, and the daemon only through
  `platform.komorebic` and `platform.state()`. It imports nothing from
  `komorebi_vm.kde`, and it names no compositor, no program and no key of a
  platform.
- It asks the operating system where a window is (`platform.os_window`) and
  never the daemon: the daemon's state is what is under test, so it is read to
  find out what the daemon believes and compared with what the OS has. A window
  is told by the title the scenario gave it.
- It waits for a state with `platform.wait_for`, not with a sleep, and compares
  frames with `platform.tolerance`, because a client may round its size to what
  suits it.
- It is checked against a fake first. `tests/fake_platform.py` is a daemon in
  memory that follows `focus`, `cycle-focus`, `move`, `cycle-move`, `stack`,
  `cycle-stack`, `unstack`, `send-to-workspace` and `focus-workspace` over three tiles, with switches to make it wrong in
  one way; a scenario's unit test runs it against the right daemon and against
  wrong ones, so that a check that cannot fail is found without a VM.

A command the fake does not know is added to the fake with the scenario, and a
method the scenario needs of a platform is added to the `Platform` protocol, to
`KdePlatform`, and to the fakes, in the same commit. Then call the scenario from
a suite, as `core-e2e` does, and run that suite on KVM: a scenario that passes
the fake and fails on KDE has found a bug in the fake, the library or the daemon.
`Snapshot.focused_title` was found wrong this way, by the `stacks` scenario.

## Recording a run with komorebi-recorder

`komorebi-recorder`'s repository is private, so it is not an input of the flake. A run that wants
it builds it elsewhere and passes the store path, evaluating with `--impure`:

```sh
KOMOREBI_RECORDER=$(nix build ~/komorebi-recorder#komorebi-recorder --no-link --print-out-paths) \
  nix build --impure .#checks.x86_64-linux.vm-core-lock -L
```

`core-e2e` and `core-lock` then record the session without video (`komorebi_vm.recorder`), print
the recorder's brief and findings into the log, and keep the session as `recording-NAME.tar.gz`
with the run's other artifacts. A run without the variable records nothing.

## Looking inside a failed run

`nix build -L` prints the guest's console, and a session test keeps its screenshots. Run the driver by hand, as
`scripts/vm-mac` does, to keep the screenshots. For an interactive session
build `driverInteractive` instead of `driver`, run it, and call `start_all()`
and `machine.shell_interact()` at its prompt. In the guest, KWin's own log is
`journalctl --user -u kwin-virtual@NAME` as the test user, and a KWin script's
`print()` goes to the same journal. The daemon's log is
`/run/user/1000/komorebi-NAME.log`. On the session node, the daemon of the
module's unit logs to `journalctl --user -u komorebi`, with colors, and to
`~/.local/share/komorebi/komorebi_plaintext.log.DATE`, without.
