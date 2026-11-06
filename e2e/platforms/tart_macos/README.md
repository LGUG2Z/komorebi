# The macOS guest

The scenarios of the shared core (`e2e/lib/komorebi_vm/core/scenarios`) run against the real
`komorebi` daemon of macOS in a virtual machine, the way they run on KDE (a NixOS test) and on Windows
(`../qemu_windows`). A scenario sees a [`Platform`](../../lib/komorebi_vm/core/platform.py); this
directory is the one for macOS.

```text
host (Apple silicon)                              guest (a clone of the golden image)
  python -m platforms.tart_macos.run                macOS, logged in, one 1280x800 display
    Guest ── tart clone / run --no-graphics ──►       Terminal (holds the grants)
    MacosPlatform ── agent protocol, TCP 7878 ──►       komorebi-e2e-agent   answers the host
                                                          komorebi            the daemon under test
                                                          komorebi-testwin    the windows to tile
```

## What a run does

For each scenario, `Guest` clones the golden image (copy on write, so it costs nothing), boots it
without a window, and waits for the agent, which answers about 20 s after the clone starts. The programs
under test are uploaded through the agent, the daemon is started by it, the shared scenario runs, and
the clone is deleted. `OUT/<scenario>/result.json` names the scenario, the commit and every check, as
the Windows runner's does; a failed run also keeps the daemon's output, the processes and a screenshot.

## Why it is built this way

- **Permissions are the hard part.** The daemon needs Screen Recording and Accessibility, which macOS
  only grants to a person's click. They are granted once to `Terminal` in the golden image and are
  inherited by every clone. The agent is started at login by a launch agent that opens Terminal on it
  (`provision.sh`), so the agent, the daemon it spawns and the test windows are all attributed to
  Terminal, and no clone asks anything. A signed binary of our own would need a grant per build.
- **The agent is a Rust program** (`crates/komorebi-e2e-agent`, the same crate and protocol as the
  Windows agent, with `macos.rs` for the verbs) because the stock image has no developer tools: `swift`,
  `clang` and `python3` there are stubs. It reads windows and displays through the accessibility API, as
  the daemon does, so a window's frame is what the application says.
- **No SSH at run time.** The vanilla image has `admin`/`admin` and SSH on, which only
  `build-golden.sh` uses.
- **The windows that are not the test's are left alone** by an ignore rule the platform adds to every
  configuration: the Terminal, and the desktop widgets of the image (Notification Center draws them as
  windows, and they are tiled otherwise).

## Limits

- **One display.** A macOS guest has one virtual display, so what needs a second one (`hotplug`,
  `monitors`, `display_change`, `unknown_monitor`) is skipped with the reason.
- **No input is injected**, so there is no pointer (`focus_follows_mouse`, drags) and no lock screen.
- **At most two guests at once on a host.** Apple's licence and Virtualization.framework both stop a
  third, so a host runs two scenarios in parallel at the most.
- **Apple hardware only.** A hosted GitHub runner is itself a virtual machine and cannot start one, so
  this does not run in CI: it runs on a Mac you have (see `docs/dev/ci.md`).
- **The wallpaper and the zoom button** are not driven yet.

## Running it

```sh
# once: the golden image (30 GB of disk; one manual step, the grants, which the script stops for)
e2e/platforms/tart_macos/build-bins.sh target/e2e-macos
e2e/platforms/tart_macos/build-golden.sh target/e2e-macos

# every time: this checkout's daemon against the image
nix run .#e2e-macos -- target/e2e-out tiling focus
```

`nix run .#e2e-macos` builds the programs under test and brings `tart` (nixpkgs has it as an unfree
package, so it is allowed for that one only). `KOMOREBI_E2E_BINS=dir` runs the programs of `dir`
(`build-bins.sh` makes one), `KOMOREBI_E2E_GOLDEN=name` another image, `KOMOREBI_E2E_SCALE=1.5` makes
every wait longer for a busy host. The image has to be built again when the agent changes, and for a
new macOS.

The unit tests of the port need no guest and run with those of the Windows one
(`python -m pytest lib/tests platforms/qemu_windows/tests platforms/tart_macos/tests`, in `e2e`, with
`PYTHONPATH=lib:.`; the `windows-e2e-tests` check does it).

Do not run it alongside a heavy build, which makes the daemon slow to answer and the waits fail.
