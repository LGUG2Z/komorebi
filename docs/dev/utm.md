# Plasma guests to develop in

The VM tests boot a machine, run a scenario and power it off. To try komorebi by hand against a
real KWin, the flake has NixOS guests with Plasma 6 that stay up. Each guest is made for one kind
of host, because the accelerator of a host (HVF on a Mac, KVM on Linux) runs guests of the host's
own architecture:

| Guest | Host | Runs under | What it is |
|---|---|---|---|
| `plasma-vm` | Apple silicon Mac | QEMU from nixpkgs, with HVF | Plasma installed, no login: a console and ssh, to run `kwin_wayland --virtual` and build komorebi from the repository |
| `plasma-vm-session` | Apple silicon Mac | QEMU from nixpkgs, with HVF | the same, plus a Plasma Wayland session that logs in by itself, komorebi running in it, and an RDP server |
| `plasma-utm` | Apple silicon Mac | [UTM](https://mac.getutm.app), with a GPU | the session guest as a disk image that boots by UEFI |
| `plasma-vm-x86_64`, `plasma-vm-session-x86_64` | Linux on x86_64 | QEMU from nixpkgs, with KVM | the first two, as `x86_64-linux` guests |
| `plasma-vm-aarch64`, `plasma-vm-session-aarch64` | Linux on aarch64 | QEMU from nixpkgs, with KVM | the first two, as `aarch64-linux` guests |
| `run-kde-x86_64`, `run-kde-aarch64` | Linux on x86_64, on aarch64 | QEMU from nixpkgs, with KVM | the session guest with a demo on it, which `run-kde` starts ([below](#the-demo-guest)) |

The guests of a Mac are `aarch64-linux`. A guest of a Linux host that has no KVM does not start:
the runner refuses, because Plasma under software emulation is too slow to try anything.

They are `nixosConfigurations` of the flake, made from
[`nix/dev/plasma-vm.nix`](../../nix/dev/plasma-vm.nix),
[`nix/dev/plasma-utm.nix`](../../nix/dev/plasma-utm.nix) and
[`nix/dev/krdp.nix`](../../nix/dev/krdp.nix), by `mkPlasmaGuest` in
[`flake.nix`](../../flake.nix), which takes the system of the guest and the packages of the machine
that runs QEMU. Each one installs komorebi with the flake's NixOS
module, `services.komorebi`, so the komorebi in a guest is the one of the commit the guest was
built from, and in the session guests the module's unit starts it at login. They carry no key,
no password and no configuration of anyone's.

In the QEMU guests the guest has a plain `virtio-gpu`, which has no 3D, so KWin composites on the
CPU (llvmpipe) and every frame then has to be encoded to reach an RDP client. That is slow. On a
Mac the QEMU in nixpkgs cannot do better: it has no 3D at all. UTM's QEMU has `virtio-gpu-gl`,
which gives the guest OpenGL through Mesa's VirGL, rendered on the Mac's GPU, and UTM shows the
guest in a window of its own, so there is no RDP in the path at all. The QEMU of an x86_64 Linux
host has and a window (GTK), at time of writing (QEMU 11.1.2 from nixpkgs, 2026-10-28), which these guests
do not use.

## What a builder has to be, on a Mac

A Mac cannot build a Linux system, so the guests are built on a builder: a machine with Nix that
builds for `aarch64-linux` and that the Mac can reach as a remote store, such as
`ssh-ng://builder@localhost`. Any builder will do: the one `nix run nixpkgs#darwin.linux-builder`
starts, a Linux machine on the network, or the one the VM tests use on a Mac
([vm-tests.md](vm-tests.md#running-a-test-on-a-mac)). A Plasma system is a closure of about 10 GB,
and the builder needs room for it and for its build: the defaults of `darwin.linux-builder` (one
core, 3 GB of memory, a 20 GB disk) are too small. Six cores and 10 GB of memory worked, at time
of writing (2026-10-27).

The examples below use the same variables as `scripts/vm-mac`:

```sh
export KOMOREBI_BUILDER=ssh-ng://builder@localhost   # the store URL of your builder
export NIX_SSHOPTS="-p 31022 -i $HOME/.ssh/builder_ed25519 -o IdentitiesOnly=yes"   # if it needs them
```

## The QEMU guests on a Mac

The system is built on the builder and copied back, and only the QEMU runner, which is a program
for the Mac, is built on the Mac. A QEMU guest runs the system of its `vmVariant` (which adds the
disk and the shared directory), not the system of the configuration itself:

```sh
guest=plasma-vm-session   # or plasma-vm
config=".#nixosConfigurations.$guest.config"
system=$(nix build "$config.virtualisation.vmVariant.system.build.toplevel" \
  --eval-store auto --store "$KOMOREBI_BUILDER" --no-link --print-out-paths)
nix copy --from "$KOMOREBI_BUILDER" --substitute-on-destination --no-check-sigs "$system"
nix build "$config.system.build.vm" --builders '' -o .vm/result-$guest
```

The guest mounts the directory `KOMOREBI_REPO` names at `/repo`, and makes its disk,
`plasma-vm.qcow2` or `plasma-vm-session.qcow2`, in the directory it is started from on its first
start, and keeps it there. `.vm` is ignored by git:

```sh
cd .vm && KOMOREBI_REPO=$PWD/.. ./result-$guest/bin/run-$guest-vm
```

The terminal is the guest's console, logged in as the user `komorebi`, who can use `sudo` with no
password. ssh is forwarded from `localhost:2222` (`plasma-vm`) or `localhost:2223`
(`plasma-vm-session`) and takes keys only: put yours in `~/.ssh/authorized_keys` from the console
once, and the disk keeps it. Both guests can run at once.

In the guest, `/repo` is the repository and `CARGO_TARGET_DIR` is in the guest's home, since a
target directory on the shared directory is slow. The session guest shows nothing on the Mac; it
is reached over RDP ([below](#rdp)).

## The QEMU guests on a Linux machine

A Linux host builds a guest itself, so there is no builder to set up. It needs Nix with flakes, KVM
(a `/dev/kvm` that the user can open, which on most distributions means the group `kvm`), and room
for the closure (about 10 GB) and for the guest's disk (20 GB, sparse). The runner is built with
its system, and started the same way as on a Mac:

```sh
guest=plasma-vm-session-x86_64   # or plasma-vm-x86_64, or the -aarch64 pair on an aarch64 host
nix build ".#nixosConfigurations.$guest.config.system.build.vm" -o .vm/result-$guest
cd .vm && KOMOREBI_REPO=$PWD/.. ./result-$guest/bin/run-*-vm
```

The console, the user, the ssh ports (2222 and 2223), the RDP port, the disk and `/repo` are as
described for a Mac. What differs is what nixpkgs' runner does on a Linux host:

- It starts one `virtiofsd` for each shared directory, as the user who runs the guest, and the
  guest mounts them by virtiofs where a Mac's guest mounts them by 9p. One of them is the host's
  `/nix/store`, so the guest has the closure without a copy of it.
- It refuses to start when `/dev/kvm` is missing or cannot be opened by the user, since the guests
  set `virtualisation.qemu.forceAccel`.

A guest has one display device, a virtio-gpu, so that KWin has a DRM device to draw on. The machine
of an x86_64 guest has a VGA of its own, which would be a second display next to the virtio-gpu,
so the guest has `virtio-vga` (a virtio-gpu that is also a VGA) in its place and `-vga none`. The
session guest shows nothing on the host, as on a Mac: it is reached over RDP ([below](#rdp)).

## The demo guest

`run-kde-x86_64` and `run-kde-aarch64` are the session guest with a demo on it, made by
[`nix/dev/run-kde.nix`](../../nix/dev/run-kde.nix) on top of `plasma-vm.nix`. komorebi is
configured by the module's typed `config` (eight workspaces with a layout each, borders and
animation) and by the sample key bindings, `docs/skhdrc.sample`, in the module's default mode. At
login the guest opens two Konsole windows, Kate and Dolphin, which komorebi tiles, and a cheat sheet
that it leaves alone: the main key bindings, `komorebic` commands to try, what KWin draws with, and
the output of `komorebic check` for the configuration the daemon runs. The VM suite `run-kde` boots
the same module on the session node.

The closure is the same for everyone. No password, key or configuration of a person is in it, and
what a person's run needs comes from the environment of the runner:

| Variable | What the guest does with it |
|---|---|
| `KOMOREBI_REPO` | the directory mounted at `/repo` |
| `RUN_KDE_STATE` | the directory mounted at `/run/run-kde`, with `rdp-password` (the password for RDP) and `ssh_key.pub` (the public key that may log in over ssh) |
| `RUN_KDE_MEMORY` | the memory in MiB, which has to be the `-m` of the same run |
| `QEMU_NET_OPTS` | the port forwards, as `hostfwd=tcp:127.0.0.1:2223-:22,hostfwd=tcp:127.0.0.1:3389-:3389` |
| `QEMU_OPTS` | the display device: `-vga none -device virtio-vga,xres=1280,yres=800`, or `-display gtk,gl=on -vga none -device virtio-vga-gl,xres=1280,yres=800` for a GPU |
| `NIX_DISK_IMAGE` | the disk image, which the runner makes when it is not there |

[`run-kde`](#the-command) sets them; to start the guest by hand, on a Linux machine:

```sh
nix build .#nixosConfigurations.run-kde-x86_64.config.system.build.vm -o .vm/result-run-kde
mkdir -m 700 .vm/state && cd .vm
ssh-keygen -q -t ed25519 -N '' -f state/ssh_key
printf %s "a password" > state/rdp-password
KOMOREBI_REPO=$PWD/.. RUN_KDE_STATE=$PWD/state RUN_KDE_MEMORY=6144 \
  QEMU_NET_OPTS=hostfwd=tcp:127.0.0.1:2223-:22,hostfwd=tcp:127.0.0.1:3389-:3389 \
  QEMU_OPTS='-cpu host -vga none -device virtio-vga,xres=1280,yres=800' \
  ./result-run-kde/bin/run-komorebi-demo-vm -m 6144 -smp 4
```

The guest differs from `plasma-vm-session` in what the runner of nixpkgs would otherwise fix when
the closure is built:

- The display device is not part of the guest (`komorebi.plasmaVm.display` is false). The same
  closure and the same disk run with the software `virtio-vga` or with `virtio-vga-gl`, which
  renders on the host's GPU through VirGL, as the runner's environment says.
- The ports are not forwarded by the guest, so that a person can choose them.
- The runner does not insist on KVM (`qemu.forceAccel` is false): `run-kde` checks for it and
  lets `--tcg` start a guest without.
- The shared memory that virtiofs needs is a memory backend of the guest's memory size, which
  nixpkgs fixes at `memorySize` in the command line of the runner. The guest makes its own, from
  `RUN_KDE_MEMORY`, so that a run can have less memory than the closure's 8 GB.
- The directory `/run/run-kde` is mounted after the boot and not in the initrd, where a mount
  under `/run` is hidden by the `/run` that the booted system mounts.
- The files in it can belong to a user of the host who is not the guest's user, and the guest
  cannot read the directory then. The unit `run-kde-credentials` copies the two files as root, to
  `/run/run-kde-guest`, where the RDP unit (`komorebi.krdp.passwordFile`) and sshd
  (`AuthorizedKeysFile`) read them.

## The command

`run-kde` starts the demo guest on a Linux machine, from a checkout: `nix run .#run-kde -- [options]`,
or `just run-kde [options]`. It is [`scripts/run-kde`](../../scripts/run-kde), which the flake wraps
with the runner of the machine's guest (`run-kde-x86_64` or `run-kde-aarch64`) written into it, as
`packages.<system>.run-kde` and `apps.<system>.run-kde`. The package names the runner and does not
depend on it, so `nix run` does not fetch 4 GB before the command can say what the machine lacks.

In order, it:

1. Checks the machine. `--doctor` prints every check, a tick or a cross or a `!`, with the fix under
   each cross and each `!`: WSL 2 or not, Nix, KVM (`/dev/kvm`, the group `kvm`, the CPU's `svm` or
   `vmx` flag, the module `kvm_amd` or `kvm_intel`, and on WSL 2 the line
   `nestedVirtualization=true` of `.wslconfig`), virtiofs (a `virtiofsd`, the runner's own when it
   is built, is started as this user with the runner's options and must make its socket: a cross
   when it does not; `/dev/fuse` is a `!`, since `virtiofsd` never opens it), memory, cores, disk, a
   window (WSLg: `WAYLAND_DISPLAY`, `DISPLAY`), Windows interop for `--rdp`, the GPU (`/dev/dxg` on
   WSL 2, a render node elsewhere, and the OpenGL renderer of the host from `glxinfo`), and the
   networking mode of `.wslconfig`. A cross stops the command and `--doctor` exits with 1; a `!`
   does not. A missing KVM is a cross, unless `--tcg` allows software emulation, which is too slow
   to try anything.
2. Builds the guest when its runner is not in the store, with `nix build` of the checkout's flake and
   the build log shown. There is no binary cache: the first run builds komorebi and downloads about
   4 GB of Plasma, which are 11 GB in the Nix store.
3. Makes the state directory, `$XDG_STATE_HOME/komorebi-run-kde` (`~/.local/state/komorebi-run-kde`),
   when it is not there: the guest's disk (20 GB, or `--size`, sparse), a random RDP password and an
   ed25519 key for ssh, all of mode 0600 in a directory of mode 0700. `--reset` deletes it. Nothing
   of it is in the Nix store.
4. Prints a banner and starts the runner with the environment of [the demo guest](#the-demo-guest):
   `-cpu host` with KVM, the repository at `/repo` (the checkout, or `KOMOREBI_REPO`), the state
   directory at `/run/run-kde`, the ports on `127.0.0.1` (ssh 2223, RDP 3389: `--ssh-port`,
   `--rdp-port`), and `-m` and `-smp` from what the machine has (`--memory`, `--cores`): the
   machine's memory less 2048 MB, between 4096 and 8192, and its cores up to 6.

The terminal is the guest's console, logged in as `komorebi`. What else the guest shows depends on
the display and on the GPU.

| `--display` | What it does |
|---|---|
| `gtk` | a window of QEMU; the default when `WAYLAND_DISPLAY` or `DISPLAY` is set |
| `rdp` | no window; the guest is reached over RDP; the default otherwise |
| `vnc` | no window; QEMU's VNC server on `127.0.0.1:5900` |
| `none` | no window; `--shell` is this with the GPU off |

`--gpu auto` uses the GPU when the display is a window, the host has a GPU device and the OpenGL of
the host (`glxinfo`) is not llvmpipe. Otherwise it does not, and the banner says why. The GPU is
`-display gtk,gl=on` with `-device virtio-vga-gl`, which renders the guest's OpenGL on the host
through VirGL; without it the guest has `virtio-vga` and renders on its CPUs. `--gpu on` with no
window starts `-display egl-headless`, which needs a DRM render node. The guest says what KWin draws
with in its cheat sheet and on the console (`GPU (virgl)` or `software (llvmpipe)`): the guest's
driver, not the host's.

On WSL 2, `--rdp` starts Windows' own RDP client once ssh says that the unit `krdp` is running:
`cmdkey.exe` stores the password for `TERMSRV/localhost`, `mstsc.exe` connects to
`localhost:3389`, and the credential is deleted when `run-kde` ends.

`--dry-run` prints the environment and the command that would be started, and starts nothing. The
script reads the machine through variables, so that a test can make up a machine:
`RUN_KDE_KVM_DEVICE`, `RUN_KDE_DXG_DEVICE`, `RUN_KDE_DRI_DIR`, `RUN_KDE_MEMINFO`, `RUN_KDE_CPUINFO`,
`RUN_KDE_OSRELEASE`, `RUN_KDE_SYS_MODULE_DIR`, `RUN_KDE_STORE_DIR`, `RUN_KDE_FUSE_DEVICE`,
`RUN_KDE_VIRTIOFSD`, `RUN_KDE_FREE_GB`, `RUN_KDE_CPUS`, `RUN_KDE_HOST_RENDERER` and
`RUN_KDE_WSLCONFIG`, besides `WAYLAND_DISPLAY` and `DISPLAY`; and `RUN_KDE_GUEST` and
`RUN_KDE_RUNNER_BIN`, which the package sets, and `RUN_KDE_QEMU_OPTS`, which adds to the options of
QEMU. The checks `run-kde-dry-run` and `run-kde-doctor`
([`nix/tests/run-kde.nix`](../../nix/tests/run-kde.nix)) run it against a WSL 2 with WSLg and a GPU,
one with WSLg and a software OpenGL, one with WSLg and no GPU device, a Linux machine with no
window, `--tcg` with no KVM, and machines that cannot run the guest (no KVM, a KVM the user cannot
open, a `virtiofsd` that exits), a `/dev/fuse` that is missing or closed, and compare the output
with the texts in [`nix/tests/run-kde`](../../nix/tests/run-kde). The check `run-kde` builds the package, which
shellchecks the script.

## The UTM disk

UTM boots `plasma-utm` from a disk image. The image is made by installing the system onto a blank
disk from a running Linux system with `nixos-install`, which needs no virtualization on the
builder: here, the `plasma-vm` guest, which has the tools for it. Build both systems first: the
`plasma-vm` runner as above, and the `plasma-utm` system, on the builder, copied back:

```sh
system=$(nix build .#nixosConfigurations.plasma-utm.config.system.build.toplevel \
  --eval-store auto --store "$KOMOREBI_BUILDER" --no-link --print-out-paths)
nix copy --from "$KOMOREBI_BUILDER" --substitute-on-destination --no-check-sigs "$system"
```

The closure goes into the guest as one file that `nix-store --export` writes, and not through the
guest's view of the Mac's store: macOS stores files whose names differ only by case under other
names (`~nix~case~hack~1`), which a guest reading the directory sees, and nix then rejects the
hashes. An export undoes that. Then boot `plasma-vm` with a blank disk of 40 GB attached:

```sh
mkdir -p .vm/utm && cd .vm
nix-store --export $(nix-store -qR "$system") > utm/plasma-utm.closure
echo "$system" > utm/plasma-utm.system
nix shell nixpkgs#qemu --command qemu-img create -f raw utm/plasma.raw 40G
QEMU_OPTS="-drive file=$PWD/utm/plasma.raw,format=raw,if=none,id=utm -device virtio-blk-pci,drive=utm" \
  KOMOREBI_REPO=$PWD/.. ./result-plasma-vm/bin/run-plasma-vm-vm
```

In the guest's console, partition the disk, which is `/dev/vdb`, and install the system onto it:

```sh
sudo parted -s /dev/vdb -- mklabel gpt mkpart ESP fat32 1MiB 512MiB set 1 esp on mkpart nixos ext4 512MiB 100%
sudo mkfs.vfat -F 32 -n ESP /dev/vdb1
sudo mkfs.ext4 -q -L nixos /dev/vdb2
sudo mount /dev/vdb2 /mnt && sudo mkdir -p /mnt/boot && sudo mount /dev/vdb1 /mnt/boot
sudo nix-store --import --store /mnt < /repo/.vm/utm/plasma-utm.closure > /dev/null
sudo nixos-install --system "$(cat /repo/.vm/utm/plasma-utm.system)" --root /mnt --no-root-passwd --no-channel-copy
sudo umount /mnt/boot /mnt && sudo poweroff
```

Back on the Mac, in `.vm`, the raw disk becomes the qcow2 that UTM is pointed at (about 12 GB
used of 40):

```sh
nix shell nixpkgs#qemu --command qemu-img convert -O qcow2 utm/plasma.raw utm/plasma.qcow2
rm utm/plasma.raw utm/plasma-utm.closure
```

## Point UTM at it

1. Install UTM (`brew install --cask utm`, or from its website).
2. New VM → **Virtualize** → **Linux**. Leave the boot ISO empty. Give it 8192 MB and 6 cores.
   (The wizard insists on a new disk; delete it in step 3.)
3. Before the first start, open the VM's settings:
   - **Drives**: remove the new disk, then **Import** `.vm/utm/plasma.qcow2` (interface VirtIO).
   - **Display**: *Emulated Display Card* `virtio-gpu-gl-pci (GPU Supported)`. New Linux VMs get
     it by default. Renderer backend *Default* (ANGLE on Metal) or *Apple Core OpenGL* (GL 4.1).
   - **Network**: *Shared Network* (the guest gets an address the Mac can reach), or *Emulated VLAN*
     with port forwards 2224→22 and 3389→3389.
   - Name it `plasma-utm`.
4. Start it. sddm logs in by itself and Plasma comes up with komorebi running.

## Using it

The VM's window is the desktop: click into it and use it. The pointer is an absolute tablet, so it
moves in and out of the window without being grabbed; if the window does capture it,
**Control+Option** releases it (UTM's default; Command+Option also works, and Control+Option
collides with VoiceOver). The Option key is Alt in the guest. macOS takes a few combinations before
UTM sees them (Control-arrow switches Spaces, and anything with Command), and those never reach the
guest. Resizing the window resizes the guest's screen, and the clipboard is shared (spice-vdagent
runs in the guest).

To check that the session is on the GPU, from a terminal in the guest (or over ssh, once a key of
yours is in `~/.ssh/authorized_keys`):

```sh
eglinfo 2>/dev/null | grep -i renderer                        # virgl (Apple M...)
journalctl --user -b -t kwin_wayland | grep -i "opengl\|renderer" | head
komorebic state | head
```

## RDP

The session guests run KRDP, Plasma's RDP server, as the user unit `krdp`, on port 3389 (forwarded
from `localhost:3389` for `plasma-vm-session`). It logs in the session's user, `komorebi`, with a
password of your choosing, which the unit reads from a file in the guest; until the file is there,
the unit does not start. In the guest:

```sh
umask 077 && mkdir -p ~/.config/komorebi-vm && cat > ~/.config/komorebi-vm/rdp-password   # type it, then Ctrl-D
systemctl --user start krdp
```

The server makes a self-signed certificate the first time it starts, so a client warns about it.
The password is passed to `krdpserver` on its command line, which every user of the guest can read
in the process list. [PLATFORM.md](PLATFORM.md#rendering-in-virtual-machines-and-over-rdp) says what
is known about RDP into the session, and what is not.

## What is not known

At time of writing (2026-10-28), the guests of a Mac, made from these files, have been built on an
`aarch64-linux` machine, and not booted: no test covers them, and `plasma-guests-eval` only
evaluates them. The `x86_64` pair for a Linux host was built and started under KVM on an `x86_64`
machine. Both guests reached the login on their serial console and had one DRM device with a render
node, and the session guest drew the Plasma desktop on its display (a screenshot taken through
QEMU's monitor) with komorebi running in the session. The demo guest, `run-kde-x86_64`, was
started under KVM with a state directory: ssh took the key from it, the RDP unit started with the
password from it, komorebi tiled the four windows, and the cheat sheet opened. `run-kde --doctor`,
`--dry-run`, `--build-only` and a start with `--display none` were run on the same machine, which has
no WSL, no window and no GPU: the guest came up with the memory and cores asked for, and it said
`software (llvmpipe)`. The GPU path was started on that machine in QEMU's GTK window on an X server
whose OpenGL is Mesa's llvmpipe. Nothing was run on WSL 2, with WSLg, with a GPU, or with
`mstsc.exe`: what the command does there is not known yet. The `aarch64` guests
for a Linux host were evaluated and not built. The RDP server is the exception to all of this: the
VM suite `rdp` runs the same unit in the test session. That KWin 6.8 runs well on VirGL under UTM
is not known either: the UTM and Mesa notes say VirGL is the way to render a Linux desktop, and
Plasma works on it for other people, but it has not been measured here.
