# Plasma guests to develop in

The VM tests boot a machine, run a scenario and power it off. To try komorebi by hand against a
real KWin, the flake has three NixOS guests with Plasma 6 that stay up, all `aarch64-linux`, for
an Apple silicon Mac:

| Guest | Runs under | What it is |
|---|---|---|
| `plasma-vm` | QEMU from nixpkgs, with HVF | Plasma installed, no login: a console and ssh, to run `kwin_wayland --virtual` and build komorebi from the repository |
| `plasma-vm-session` | QEMU from nixpkgs, with HVF | the same, plus a Plasma Wayland session that logs in by itself, komorebi running in it, and an RDP server |
| `plasma-utm` | [UTM](https://mac.getutm.app), with a GPU | the session guest as a disk image that boots by UEFI |

They are `nixosConfigurations` of the flake, made from
[`nix/dev/plasma-vm.nix`](../../nix/dev/plasma-vm.nix),
[`nix/dev/plasma-utm.nix`](../../nix/dev/plasma-utm.nix) and
[`nix/dev/krdp.nix`](../../nix/dev/krdp.nix). Each one installs komorebi with the flake's NixOS
module, `services.komorebi`, so the komorebi in a guest is the one of the commit the guest was
built from, and in the session guests the module's unit starts it at login. They carry no key,
no password and no configuration of anyone's.

The QEMU in nixpkgs has no 3D, so in the QEMU guests KWin composites on the CPU (llvmpipe) and
every frame then has to be encoded to reach an RDP client. That is slow. UTM's QEMU has
`virtio-gpu-gl`, which gives the guest OpenGL through Mesa's VirGL, rendered on the Mac's GPU, and
UTM shows the guest in a window of its own, so there is no RDP in the path at all.

## What a builder has to be

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

## The QEMU guests

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

At time of writing (2026-10-27), the guests made from these files have been built on an
`aarch64-linux` machine, and not booted: no test covers them, and `plasma-guests-eval` only
evaluates them. The RDP server is the exception: the VM suite `rdp` runs the same unit in the test
session. That KWin 6.8 runs well on VirGL under UTM is not known either: the UTM and Mesa notes say
VirGL is the way to render a Linux desktop, and Plasma works on it for other people, but it has not
been measured here.
