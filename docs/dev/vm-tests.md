# VM tests

The unit tests run against a fake desktop. They cannot say whether KWin does
what the daemon asks of it, so a second kind of test boots a NixOS virtual
machine with Plasma in it, starts a real KWin, and drives it. This page says how
they are laid out, how to run them, and how to add one.

## What runs where

| Where | How | Accelerator |
|---|---|---|
| CI, `x86_64-linux` | `nix build -L .#checks.x86_64-linux.vm-NAME` | KVM |
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

TCG is software emulation. It is slow enough that only short suites are worth
running that way, and the library's `scale` argument (below) is 8 for it.

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
  is.

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
needs pixels. The choice of login manager is made in
[`nodes/session.nix`](../../nix/tests/vm/nodes/session.nix) and nowhere else;
the file says how to switch to sddm, the other one that has been seen to work.
Its display is 1280x800, because screenshots are taken of the machine's one
display device: the `virtio-gpu-pci` that the test driver adds to every aarch64
guest, and the default VGA of an x86_64 one. The node sets
`komorebi.vm.renderNode` to false, so that an x86_64 guest does not get a second
device: KWin would draw on it, and the screenshot would show the other one.

On aarch64 under HVF, at time of writing (2026-10-27), the session has the
Wayland socket after 11 to 19 seconds and a painted desktop after 35 seconds.

## The library

Suites import from `komorebi_vm`, in
[`nix/tests/vm/lib`](../../nix/tests/vm/lib). It does not depend on the test
driver. It depends on the small `Machine` protocol, and its unit tests pass a
fake machine, so the `vm-lib` check tests it without booting anything. The
check also runs `ruff` and `ty` over it. The driver type checks every
`test.py` with `ty` and `ruff` too, so a typo in a suite fails the build, not
the run.

- `Checks` counts checks and fails once, at the end, so that one failure does
  not hide the ones after it. `finish()` prints `passed: N failed: M`.
- `Kde` is one KWin session. `Kde.headless(machine, name, size=, outputs=,
  scale=)` starts the units and waits for KWin to own its name on the private
  bus. `wid`, `rect`, `title` and `active` read windows through `kdotool`,
  `outputs` counts the enabled outputs through `kscreen-doctor`, `spawn` starts
  a client, and `wait_for(predicate, timeout)` polls, with the timeout
  multiplied by `scale`.
- `Kde.session(machine, scale=)` attaches to the session node's own Plasma
  session once its desktop is up, and `shot(name)` returns a screenshot as a
  Pillow image, keeping `NAME.png` in the output directory.
- `activate`, `minimize` and `close` act on a window through `kdotool`, as the
  user would, and `wid_by_class` finds a window by its app id or X11 class.
  `Kde.headless(..., xwayland=True)` gives the session XWayland, for X11
  clients.
- `start_komorebi(*args)` starts the daemon in the session, with the module's
  `execStart` and `args` after it (not `--log-level`, which `execStart` has),
  `stop_komorebi()` stops it with `SIGTERM`, `komorebi_log()` returns its log
  since it was last started, and `script_loaded()` asks KWin whether it runs
  the bridge.
  `restart_kwin()` stops the session's KWin and starts a new one on the same
  bus, as when the compositor of a running session crashes, and waits for it.
- `komorebic(*args)` runs `komorebic` in the session and returns what it
  printed, failing the test if it fails. `state()` is the daemon's state as
  `komorebic state` prints it, `managed(workspace)` the number of windows the
  daemon has on a workspace of the first monitor, `managed_all()` the number it
  has on every workspace of every monitor, and `focused_title(workspace)` the
  title of the window it has focused there.
- `daemon` reads the daemon's view of the desktop from that log, at the debug
  level the nodes set: `daemon.events(log)` lists the window events it handled,
  in order, `daemon.window_ids(log)` maps a window's title to the daemon's id
  for it, and `daemon.requests(log)` returns the frame the daemon last asked KWin
  for, for each window.
- `monitor_bridge(path)` records the bridge's traffic on the session bus with
  `dbus-monitor`, and `latency.rounds` times each batch of commands in it, from
  the daemon's answer to the last frame KWin reports for the windows it moved.

Poll for what the test needs with `wait_for`. A fixed sleep is too long on
KVM and too short on TCG.

Two traps are worth knowing about before writing a screenshot test:

- A splash screen is not a blank screen. KSplash has more than a thousand colors
  on it, so "the screenshot is not blank" passes while Plasma is still
  starting. `Kde.session` waits for the splash process to exit, and a test
  should check a pixel it knows, such as the middle of a terminal.
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
   minutes. `slow` suites run on `master` and every night.
3. Run it: `just vm NAME`.
4. If the test covers behavior the daemon has, write the Rust test that covers
   the same behavior against the fake desktop in the same commit, and say in
   the VM test's first comment what only a real KWin can tell.

Add `git add -N` for new files first: a flake only sees files git knows about.

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
