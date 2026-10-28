# Trying komorebi in a virtual machine

This page is for someone who wants to see komorebi tile windows in a Plasma session without setting
up a machine for it. One command starts a NixOS virtual machine with Plasma, komorebi and a few
windows that komorebi tiles, in a window of your own desktop, and you try the key bindings by hand.
It is for a Linux machine with KVM, and the steps below are for NixOS in WSL 2 on Windows 11, which
is the machine it was written for. Nothing on this page has been run on WSL 2 yet: what was run, and
what was not, is [at the end](#what-is-known).

The virtual machine is the demo guest of [`nix/dev/run-kde.nix`](../nix/dev/run-kde.nix), and the
command that starts it is [`scripts/run-kde`](../scripts/run-kde). [Plasma guests to develop
in](dev/utm.md) says how they are made.

## What you need

- A checkout of this repository. The repository is private, so the command runs from a checkout and
  not from `github:`.
- Nix with flakes.
- KVM, which on WSL 2 is nested virtualization: the guest is a virtual machine inside WSL's own.
- About 16 GB free in the Nix store for the first run, and room for the guest's disk, which is 20 GB
  at most and grows as the guest writes to it.
- At least 4 GB of memory for the guest, and 8 GB if you can: WSL gets half of the machine's memory
  unless you say otherwise.

There is no binary cache. The first run builds komorebi from source and downloads about 4 GB of
Plasma and the rest of the guest, which are 11 GB in the Nix store, and the command says so before
it starts. [The first run](#the-first-run) says how long that took where it was measured.

### NixOS-WSL

In `%UserProfile%\.wslconfig` on the Windows side, under `[wsl2]`:

```ini
[wsl2]
memory=12GB
nestedVirtualization=true
```

`nestedVirtualization=true` is what gives WSL a `/dev/kvm`, and on an AMD machine it needs SVM turned
on in the firmware setup. Then run `wsl --shutdown` in PowerShell, so that WSL starts again with the
new file.

In the NixOS configuration of the WSL machine:

```nix
{
  # the Windows GPU driver, for OpenGL in WSL (the GPU, below)
  wsl.useWindowsDriver = true;
  # /dev/kvm belongs to this group
  users.users.<you>.extraGroups = [ "kvm" ];
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
}
```

`<you>` is your user. Switch to it with `sudo nixos-rebuild switch` and open a new shell. WSLg, which
shows the guest's window, comes with WSL 2 on Windows 11: `wsl --update` in PowerShell brings it up
to date.

### Checking

```sh
nix run .#run-kde -- --doctor
```

`--doctor` prints each prerequisite as a tick, a cross or a `!`, with the exact fix under every cross
and every `!`, and exits with 1 if there is a cross. A missing KVM is a cross, and so is a
`virtiofsd` that does not start: the guest mounts its `/nix/store` through one. A `!` is something
that makes the guest worse and does not stop it: less memory than is comfortable, no GPU, no way to
open Windows' RDP client.

If it does not say why the guest cannot start, `nix run .#wsl2-probe` boots a tiny guest with and
without KVM, opens a window, and mounts a directory by virtiofs, which is what the guest does with
its store; it prints the fix for each thing that is wrong, and what to paste back
([Probing a NixOS-WSL2 machine](dev/wsl2-probe.md)).

## The command

```sh
nix run .#run-kde
```

or `just run-kde`, from the checkout. It checks the machine as `--doctor` does, builds the guest when
it is not in the store, makes the state directory, prints a banner and starts the guest. The banner
says what you will see, the RDP address and password, the ssh command, how to quit, and which
graphics mode it chose and why.

The terminal you ran it in becomes the guest's console, logged in as `komorebi`, who can use `sudo`
with no password. To quit, run `sudo poweroff` there, or press Ctrl-a x in the terminal to stop QEMU
at once.

The options that matter:

| Option | What it does |
|---|---|
| `--doctor` | checks the machine and stops |
| `--dry-run` | prints what would be started, and starts nothing |
| `--gpu auto\|on\|off` | whether the guest renders on the host's GPU (default: auto) |
| `--display gtk\|rdp\|vnc\|none` | a window, or no window (default: a window when there is one) |
| `--rdp` | on WSL 2, opens Windows' own RDP client when the guest is up |
| `--memory MB`, `--cores N`, `--size GB` | the sizes of the guest, which default to what the machine has |
| `--reset` | deletes the guest's disk, password and key |
| `--tcg` | allows software emulation when there is no KVM, which is too slow to try anything |

`run-kde --help` has the rest.

## The first run

The first run is the long one, and only the first. komorebi is built from source, because there is
no binary cache for it, and Plasma and the rest of the guest come from `cache.nixos.org`: 1944 paths,
3.9 GiB to download and 11.3 GiB in the Nix store, with the sources and derivations that Nix keeps
about 16 GB in all.

At time of writing (2026-10-28), on a server with 32 cores and a connection of about 3 Gbit/s, from
a Nix store with nothing in it and only `cache.nixos.org` as a source, building the guest took 209
seconds (3 min 29 s): the download took about 2 minutes of that, and the build of komorebi's
dependencies and of komorebi finished at 175 and 201 seconds. The same two builds, repeated on the
first N cores of that machine:

| Cores | komorebi's dependencies | komorebi |
|---|---|---|
| 32 | 77 s | 26 s |
| 16 | 64 s | 26 s |
| 8 | 106 s | 27 s |
| 4 | 199 s | 33 s |

The dependencies are the same for every commit and are built once; komorebi itself takes about half
a minute whatever the number of cores, and is built again for each commit. With Plasma already in the
store, `run-kde --build-only` on a new commit took 50 s on that machine, of which komorebi was 26 s,
and the guest was then ready to start. A run that has nothing new to build starts the guest at once.

What a first run takes on a WSL 2 machine is an estimate, from these numbers. The download is the
part that no number of cores helps: 3.9 GiB takes about a minute at 500 Mbit/s, 5 minutes at 100
Mbit/s and 10 minutes at 50 Mbit/s. Add 2 to 4 minutes for the dependencies on 8 to 4 cores, a
minute for komorebi and for Nix to evaluate the guest, and the time that Nix needs to write 11.3 GiB
into WSL's virtual disk. That is about 6 to 8 minutes on a fast connection and 8 or more cores, and
15 minutes or more on a slow one. Three things make it less certain: the machine measured has a
server's cores and an NVMe disk, WSL's disk is a virtual disk on the Windows drive, which a virus
scanner on the Windows side can slow down, and a NixOS-WSL that is in use has some of the paths
already, so fewer than 1944 are fetched. None of it has been measured on WSL 2.

## What you see

A window of QEMU opens with a Plasma session in it, 1280x800. At time of writing, on a machine with
KVM and no GPU (with `--display none`, so that a screenshot could be taken over QEMU's monitor), the
guest answered ssh 12 seconds after the command started, and komorebi had tiled four windows after
28 seconds: two terminals, Kate and Dolphin, with a border around each. Nested virtualization is
slower than that machine's KVM, and how much slower is not measured. A fifth window, the cheat sheet,
floats in the middle: komorebi leaves it alone. It has the key bindings, `komorebic` commands to try,
what KWin draws with, and the output of `komorebic check`. Press `q` to close it, and run
`run-kde-cheat-sheet` in a terminal to open it again.

The repository is mounted in the guest at `/repo`, and the guest's disk keeps what you do in it
between runs, in the state directory.

### The key bindings

komorebi reads the bindings of [`docs/skhdrc.sample`](skhdrc.sample), in the module's default mode,
where the daemon handles every binding. Alt is either Alt key. The ones to start with:

| Keys | What they do |
|---|---|
| Alt + h, j, k, l | focus the window to the left, below, above, right |
| Alt + Shift + h, j, k, l | move the window that way |
| Alt + Shift + Return | promote the window to the biggest tile |
| Alt + t | take the window out of the tiles, and back in |
| Alt + Shift + f | toggle monocle: the window fills the screen |
| Alt + 1 to 9 | show workspace 1 to 9, which each have a layout of their own |
| Alt + Shift + 1 to 9 | move the window to that workspace |
| Alt + x, Alt + y | flip the layout horizontally, vertically |

[Key bindings](usage/keybindings.md) says how komorebi reads a file like this one. Whether Windows
or WSLg takes any of these combinations before the QEMU window sees them is not known. QEMU's window
grabs the keyboard and the pointer with Ctrl + Alt + g.

## The GPU

KWin in the guest draws with the CPU (llvmpipe) unless the host's GPU does the work for it. With
`--gpu auto`, which is the default, the command uses the GPU when the display is a window, the host
has a GPU device (`/dev/dxg` on WSL 2) and the host's OpenGL is not llvmpipe. Then the guest has a
`virtio-vga-gl` device and the window is QEMU's GTK with OpenGL, and the guest renders through
VirGL on the host's GPU. When it does not, it says why in the banner and starts with a software
`virtio-vga`.

`--gpu on` and `--gpu off` override the choice, and `--gpu off` is the way back if the GPU path gives
a black window or a guest that does not start. The guest says which it got, in the cheat sheet and
on the console: `GPU (virgl)` or `software (llvmpipe)`. That is the guest's driver, not the host's:
the guest cannot tell a GPU from a host that draws in software.

On WSL 2 the command also sets what Mesa and libva need to find the Windows driver, in the process
that runs QEMU, unless you have set it yourself: `GALLIUM_DRIVER=d3d12`, `LD_LIBRARY_PATH` with
`/usr/lib/wsl/lib` first, and `LIBVA_DRIVER_NAME` and `LIBVA_DRIVERS_PATH` for the video driver of
Mesa for it. With them the host's GPU encodes the H.264 that KRDP sends over RDP: the guest's VA-API
lists an encoder, and KRDP uses it. Without the video driver (`/run/opengl-driver/lib/dri/d3d12_drv_video.so`,
which `wsl.useWindowsDriver = true;` brings on NixOS-WSL) the guest encodes on its CPUs, and
`run-kde --doctor` says so.

## RDP

The guest also runs an RDP server, which is the way in when there is no window. The banner has its
address, `127.0.0.1:3389`, the user, `komorebi`, and a password that was made for this state
directory. On WSL 2, `--rdp` opens Windows' own client, `mstsc`, once the guest has a session, with
the password stored for the run and deleted when it ends. If Windows' own Remote Desktop service
uses port 3389, give the guest another with `--rdp-port 3390`.

KRDP sends H.264 video only. FreeRDP built with OpenH264 shows nothing of it
([PLATFORM.md](dev/PLATFORM.md#rdp-into-the-session)), and nobody has tried Windows' client yet, so
the window of QEMU is the first way to look at the guest and RDP the second.

## Starting again

`run-kde --reset` deletes the state directory, `~/.local/state/komorebi-run-kde`: the disk, the RDP
password and the ssh key. The next run makes new ones, and the guest starts as it was built.

## Working on komorebi in the guest

`/repo` is the checkout, and `CARGO_TARGET_DIR` is in the guest's home, since a target directory on
a shared directory is slow. The guest has `rustc`, `cargo`, a C compiler and `git`. To run the
daemon you built in place of the one the module started:

```sh
systemctl --user stop komorebi
cd /repo
cargo run --bin komorebi --locked -- --log-level debug
```

The module's unit starts again with the next login. Building komorebi in this guest has not been
tried.

## When something is wrong

| What you see | What it is | What to do |
|---|---|---|
| `run-kde` says KVM does not exist | WSL has no nested virtualization, or SVM is off | `nestedVirtualization=true` in `.wslconfig`, SVM in the firmware setup, then `wsl --shutdown` |
| `run-kde` says KVM cannot be opened | the user is not in the group `kvm` | `users.users.<you>.extraGroups = [ "kvm" ];`, switch, open a new shell |
| `run-kde` says virtiofsd does not start | `virtiofsd` could not serve a directory as this user; the lines it printed say why | a long `$TMPDIR` makes a socket path of more than 107 bytes: `TMPDIR=/tmp nix run .#run-kde`; otherwise `nix run .#wsl2-probe` and paste what it prints |
| `run-kde` says `/dev/fuse` is missing or closed (a `!`) | the kernel has no FUSE, or the user cannot open its device; `virtiofsd` does not use it | nothing is needed for the guest; the fix it prints is for other FUSE programs |
| No window opens | WSLg is not there | `wsl --update` in PowerShell; or `--display rdp` |
| The window is black, or the guest never shows a desktop | the GPU path | `--gpu off`; wait a minute first, the first start is slow |
| Everything is slow | the guest draws with the CPU, or has too little memory | `run-kde --doctor`; `memory=` in `.wslconfig` |
| The guest stops with no memory | WSL has less than the guest | `--memory 4096`, or `memory=` in `.wslconfig` |
| `nix` runs out of space | the closure is about 11 GB, and the first run needs 16 GB | `nix-collect-garbage -d`, or give the WSL disk more room |
| `mstsc` cannot connect | something else has port 3389, or WSL forwards no localhost ports | `--rdp-port 3390`; remove `networkingMode=mirrored` from `.wslconfig` |
| `ssh` asks for a password | the guest takes the key of the state directory and no password | `ssh -i ~/.local/state/komorebi-run-kde/ssh_key -p 2223 komorebi@127.0.0.1` |
| The windows are not tiled | komorebi has not found the panel yet | wait; `komorebic retile` in a terminal |

## What is known

At time of writing (2026-10-28), the demo guest was started under KVM on an x86_64 machine that has
no WSL, no window and no GPU, from a fresh clone of the repository: `--doctor`, `--dry-run` and
`--build-only` worked, and a start with `--display none` made the state directory (files of mode
0600), booted the guest, let ssh in with the generated key, tiled the four windows, and showed the
cheat sheet. A screenshot taken over QEMU's monitor shows the Plasma desktop with the four tiles and
the cheat sheet on top, `komorebic check` in the guest reported no error and no warning, and `sudo
poweroff` ended `run-kde`. The VM suite `run-kde` boots the same module in CI. The GPU path was
started on that machine in a QEMU window on an X server whose OpenGL is Mesa's llvmpipe, and the
guest drew with VirGL. What is not known is
everything that is particular to WSL 2: whether `/dev/kvm` is there with nested virtualization
on an AMD machine, whether QEMU's window opens through WSLg and how fast it is, whether the
Windows GPU reaches OpenGL in the host so that the GPU path renders on it, whether `virtiofsd` and
QEMU's shared memory work under WSL 2 (the guest mounts the repository and the host's Nix store with
them; on the machines without WSL that the chain was tried on, a guest mounted both), and what
Windows' RDP client shows of KRDP. [PLATFORM.md](dev/PLATFORM.md#wsl-2) lists them.
