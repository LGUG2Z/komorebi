# The Windows guest for the neutral e2e scenarios

The scenarios in `e2e/lib/komorebi_vm/core` (vendored from the KDE port with
`scripts/sync-e2e-core` there) run against a throwaway Windows 11 guest under
QEMU. Findings and measurements are in `W0.md` and `W1.md`.

## Needs

- Linux with KVM (nested is enough: WSL2 with `nestedVirtualization=true`).
- The Windows 11 Enterprise Evaluation ISO (`go.microsoft.com/fwlink/?linkid=2289031`).
- Nix. `nix run .#e2e-windows` brings QEMU, swtpm, an OVMF with TPM support
  and the binaries under test.

## Build the golden image, in two layers

Windows Setup is about 97% of building an image (measured: 18 minutes of a
18.2 minute build), so the image is two layers, a qcow2 chain.

The **base** is the unattended install, up to a desktop with auto-login, the
Visual C++ runtime and the guest agent. Build it once, and again for a new ISO,
when the agent needs more in the image, or when the 90-day evaluation runs
out (about 20 minutes, more under load):

```sh
nix build .#komorebi-e2e-agent -o agent
build-base.sh win11.iso agent/bin/komorebi-e2e-agent.exe base
```

The **golden** layer goes on top of it in about 2.5 minutes, most of it the
boot: policies, Defender, the display driver and the like, provisioned
through the agent (`provision.ps1`), so a change to those costs minutes, not
a reinstall. Run it from `e2e/` with the nix app's environment (`PYTHONPATH=lib:.`,
`OVMF_FD`, qemu and swtpm on PATH):

```sh
build-golden.sh base golden QXLDOD_DIR
```

The build script needs `python` on PATH as well (`nix shell nixpkgs#python3`).
Measured from these scripts: 127 s on a loaded host.
`QXLDOD_DIR` is virtio-win's `qxldod/w10/amd64` (Red Hat's signed QXL display
driver). Without it the guest has one display and `hotplug` and the
two-display `commands` sweep cannot run. `golden/disk.qcow2` is backed by
`../base/disk.qcow2` through a relative path: move the two together, or
`qemu-img rebase -u` the golden one. Both builds need the guest to have
internet (the Visual C++ runtime is downloaded in it).

## Run scenarios

```sh
E2E_GOLDEN=golden nix run .#e2e-windows -- OUT SCENARIO...
```

Shared scenarios (from `e2e/lib`, vendored from kfde; their checks print as
`[shared]`):

- tiling and focus: `tiling`, `focus`, `moving`, `stacks`, `workspaces`,
  `monocle`, `closing`, `layouts`, `floating`, `minimizing`, `maximizing`,
  `workspace_moves`, `promoting`, `flipping`, `pausing`, `tiling_off`,
  `scrolling`, `padding`
- resizing: `resize` (a pointer drag), `resize_commands`
- rules and config: `rules`, `rules_class`, `rules_exe`, `manage_rules`,
  `state_fidelity`, `placement`, `startup_rules`, `config_reload`
- lifecycle: `lifecycle`, `lifecycle_hidden`, `locking`
- displays: `hotplug`, `commands` (also the sweep of every command; with
  `SWEEP_TWO_DISPLAYS=1` it adds a second display), `unknown_monitor`
- `focus_follows_mouse` (needs the pointer, on the first display only)

What only Windows checks, run after the tiling windows with `[windows]`
labels, from `extras/`: `windows_padding` (the border's 7 px),
`windows_cloaking`, `windows_config_file`, `windows_tracking`,
`windows_second_display` and `windows_display_change` (the mode of the QXL
display changes while windows are on it).

Every scenario gets its own guest on a fresh overlay of the golden
image; each needs about 4 vCPU and 6 GB. `OUT/<scenario>/result.json` names the
scenario, the commit under test (`KOMOREBI_E2E_COMMIT`), every check (with its
scope), and the seconds each phase took.

| Variable | Effect |
|---|---|
| `KOMOREBI_E2E_STATE=DIR` | restore every guest from a state saved in DIR (made on the first run, and again when the golden image, QEMU or `guest.py` changed): about 3 s instead of 50 to 90 s; the state is about 2.7 GB and the guest then has an AHCI disk |
| `KOMOREBI_E2E_SCALE=1.5` | every wait of a scenario is that many times as long, for a busy host |
| `KOMOREBI_E2E_BINS=DIR` | (the nix app) test the binaries in DIR, made by `build-ref.sh REF DIR`, instead of this checkout's |
| `KOMOREBI_E2E_KEEP_GUEST=1` | keep each guest's run directory (overlay, TPM state), which is deleted at exit by default |
| `SWEEP_TWO_DISPLAYS=1` | `commands` adds a second display first |
| `KOMOREBI_E2E_LOG_LEVEL` | the daemon's log level (`debug`, `trace`) |
| `KOMOREBI_E2E_RECORDER=DIR` | record the run with komorebi-recorder (folder with `komorebi-recorder.exe`); the session comes back as `recording.zip` |
| `KOMOREBI_E2E_CAPTURE=DIR` | save raw `komorebic state` fixtures |
| `E2E_ONLY=word,word` | run only the steps of a scenario whose printed name (`module.function`) contains one of the words, for a rerun of one step: `E2E_ONLY=at_cursor` with `monitors`; a step the runner needs to open the windows always runs (`KFDE_ONLY` is the older name) |
| `RESIZE_DX` | the drag of `resize`, in pixels |

Keep the output and run directories on a short path: the guest's sockets are
in them and a unix socket path is limited to about 108 bytes.

## Testing another commit or branch

```sh
build-ref.sh origin/next bins        # komorebi, komorebic and testwin of that ref
KOMOREBI_E2E_BINS=bins E2E_GOLDEN=golden nix run .#e2e-windows -- OUT SCENARIO...
```

`bins/commit` has the full hash, which the runner puts in every `result.json`.
A known-failure marker of the runner applies only when that commit does not
contain the fix of the bug it is for (`git merge-base --is-ancestor`, in
`run.py`): `unknown_monitor` (fixed by `837ca257`) and the integer case of
`placement` (`3b424dd7`) are known failures on `0ffc502c` and plain passes on
komorebi's `next` (there, a marker that still applied would fail the suite, to
say it can go).

## Known failures and limits

- On a commit without the fix, `unknown_monitor` (kfw drops the focused window
  when the target monitor does not exist) and the integer case of `placement`
  (the daemon exits at start) are known failures: the suite prints
  `known failures: N`, and fails once the bug is fixed and the marker holds.
- One step is a known difference of kfw, kept for now (`KNOWN_STEPS` in
  `run.py`; its failing checks print as `known failure` and a step that stops
  failing says the marker can go): `hotplug.user_move` (kfw relocates a window
  to another monitor only when a drag ends, `MoveResizeEnd`).
  `maximizing.user` is a known failure only on a build without `3fa3d429`
  (kfw did not track a user's maximize); master passes it.
- `hotplug` failed about two runs in three on `0ffc502c` (an unplugged
  monitor stayed in the list); `next` has the fix (`5c8e4770`).
- By design, not bugs: a minimized window is not taken up when the daemon
  starts, and a rule's exe id has the extension (`exe_id` spells it).
- Under heavy host load the daemon can answer `komorebic state` slowly enough
  to time out; a run that overlaps other VM suites is suspect.
- The QMP tablet reaches the first display only, so `platform.pointer`
  cannot act on a second one.
- Only the second (QXL) display lists modes, so only it can change mode; the
  first is the basic display adapter, with one. A change of the Windows scale
  (DPI) needs a sign-out and is not tried.

## Pieces

`guest.py` (QEMU, swtpm, the overlay), `qmp.py` (keys, pointer, screenshots),
`agent.py` and `../../guest/agent` (windows, processes and files in the guest
over TCP), `windows_platform.py` (the shared `Platform` protocol), `run.py`
(the runner), `extras/` (what only Windows checks), `display.ps1` (changes the
mode of a display), `w1_*.py` (probes, each prints what it saw).
