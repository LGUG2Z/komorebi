# Probing a NixOS-WSL2 machine

A Plasma guest that runs komorebi can be started on a Windows machine from a
NixOS virtual machine in WSL2: QEMU with KVM boots the guest, and its window is
shown through WSLg. Whether a given machine can do that depends on five things
that nothing but that machine can answer: whether WSL2 passes KVM through,
whether WSLg opens a QEMU window, whether the Windows GPU reaches OpenGL in
WSL, whether Windows can reach a port that the guest listens on, and whether the
guest can mount the host's `/nix/store` by virtiofs. The probe answers them in
about two minutes, and prints what to change for each answer that is not a yes.

It changes nothing on the machine. It reads files and system settings, boots a
tiny guest under QEMU that powers itself off, opens two windows for a few
seconds each, starts `virtiofsd` for a few seconds, and listens on two local
ports until it ends.

## Running it

From a checkout of this repository, in the NixOS-WSL2 shell, with flakes
enabled:

```sh
nix run .#wsl2-probe
```

`just wsl2-probe` runs the same command, and arguments for the script go after
it (`just wsl2-probe --hold 15`). Nix brings QEMU, `virtiofsd`, Mesa's
`eglinfo` and `glxinfo`, `socat` and the guest, about 1.6 GiB in the Nix store
as of 2026-10-28, most of it QEMU's and Mesa's dependencies. The guest is a
kernel and a static busybox from the binary cache, and the two kernel modules
that virtiofs needs, so nothing is compiled; building the module closure also
downloads the kernel's module tree (about 150 MB), which the probe does not keep.

While it runs, look at the Windows desktop: a QEMU window opens twice for 8
seconds (P2, then P3), and after each the script asks whether it appeared. The
answer is one of the things the script cannot find out itself.

| Option | Effect |
|---|---|
| `--only P1,P3` | runs only those probes (P0 always runs: the others read what it found) |
| `--hold SECONDS` | how long a window stays open (default 8) |
| `--yes` | answers yes to "did a window appear?" without asking |
| `--no-prompt` | asks nothing; this is also the case when stdin is not a terminal |
| `--dry-run` | runs P0 and P5, which only read, and prints what the others would start |
| `--report FILE` | where to write the report (default `wsl2-probe-report.txt` in the current directory) |

The exit status is 0 when the probes ran, whatever they found, and 2 for a
mistake in the options.

## Reading the output

Every line that carries a result starts with one of these:

| Tag | Meaning |
|---|---|
| `[PASS]` | the check passed |
| `[WARN]` | it works, but not as it should, or the check cannot say |
| `[FAIL]` | the thing the real guest needs is missing |
| `[SKIP]` | the check did not run, and the line says why |
| `[INFO]` | a value that was read or measured; the summary repeats it |

A `WARN` or a `FAIL` is followed by a `fix:` block with the exact change: the
lines for `%USERPROFILE%\.wslconfig`, an option of the NixOS configuration, or a
command for PowerShell. The checks have numbers (`P1.2`), and the fix of a
check often names another (`see P1.1`).

The output ends with a block between two lines of equals signs, headed
`PASTE THIS`: one verdict per probe, one line per check, and every fact the
probes read or measured. That block is what to send back. The whole output is in
`wsl2-probe-report.txt`, without colors.

## The probes

### P0: the environment

| Check | What it looks at | Verdict |
|---|---|---|
| P0.1 | `WSL_DISTRO_NAME`, `/run/WSL` and `/proc/version` | `PASS` on WSL2; `WARN` on WSL1 or when this is not WSL at all, where the probes can only report what they find |
| P0.2 | `nix config show experimental-features` | `PASS` when flakes are on; `WARN` with the NixOS option when not |
| P0.3 | `trusted-users` against the user and its groups | `PASS` when the user is trusted; `WARN` with the NixOS option when not |

The facts are the kernel, PID 1, `/etc/os-release` and `nixos-version`, the
`nixos-wsl` input of `/etc/nixos/flake.lock` if there is one, memory, the number
of processors, the free disk in `/nix`, the home directory, the directory the
real guest would keep its state in, the current directory and `/mnt/c`, the CPU
vendor, model and virtualization flag, the Nix version and substituters, and,
through interop, the Windows build, `wsl.exe --version` and the `.wslconfig` of
the Windows user. When interop is off, the Windows facts are missing and P4.1
says how to turn it on.

### P1: KVM

| Check | What it looks at | Verdict |
|---|---|---|
| P1.1 | the `svm` (AMD) or `vmx` (Intel) flag of `/proc/cpuinfo` | `FAIL` with the `.wslconfig` lines for nested virtualization when neither is there; `INFO` on a CPU that has neither flag by design |
| P1.2 | `/dev/kvm` exists and the user can open it read-write | `FAIL` when it is missing (nested virtualization) or closed to the user (the `kvm` group, with the exact `extraGroups` line) |
| P1.3 | a guest booted under `-accel kvm -cpu host` and the loop it ran | `PASS` with the loop time and the time of the whole boot; `FAIL` with QEMU's error and a fix chosen from it |
| P1.4 | the same guest under `-accel tcg`, software emulation | `PASS` with the times; `FAIL` means QEMU does not work here at all |
| P1.5 | TCG's loop time divided by KVM's | `PASS` at 3 or more, `WARN` from 1.5, `FAIL` below |

The facts are the nested-virtualization setting of `kvm_amd` or `kvm_intel`, the
QEMU version, the guest's CPU model, memory and flags (whether it sees `svm` or
`vmx` itself, and `hypervisor`, `avx`, `avx2`, `aes`), and the loop timed on the
machine itself with the same busybox.

The loop is 600000 iterations of an `ash` counting loop, timed from
`/proc/uptime` (a resolution of 10 ms, so the loop has to take about a second).
KVM runs the guest's instructions on the CPU, so its time is about that of the
machine itself; TCG translates them, and is several times slower. On a Xeon
6975P with KVM, at time of writing (2026-10-28), the loop took 970 ms on the
machine, 980 ms in a KVM guest and 11370 ms under TCG, 11.6 times as long. A
ratio under 3 means the acceleration is partial or absent, and the figure to
paste is `kvm_vs_tcg`.

### P2: display

| Check | What it looks at | Verdict |
|---|---|---|
| P2.1 | `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_RUNTIME_DIR`, `/mnt/wslg` and the sockets in them | `PASS` when the shell can reach a display; `WARN` when only WSLg's defaults (`/mnt/wslg/runtime-dir/wayland-0`, `:0`) can, which the probe then uses for its windows; `FAIL` on WSL with none (`WARN` elsewhere) |
| P2.2 | `-display gtk -device virtio-vga` for `--hold` seconds, with the guest printing a line a second | `PASS` when QEMU started, the guest ran to its end with the window up, and QEMU exited 0; `FAIL` with QEMU's error otherwise. When the window system the toolkit picked fails and both are available, each is tried by name (`GDK_BACKEND`) |
| P2.3 | your answer to "did a window appear?" | `PASS` for yes; `FAIL` for no; `INFO` when it was not asked (`--no-prompt`, no terminal) or `--yes` answered it |

A line says when the framebuffer's frame rate was not measured: the guest is a
text console, so there is no moving picture to count frames of.

### P3: GPU

| Check | What it looks at | Verdict |
|---|---|---|
| P3.1 | `/dev/dxg`, the device through which WSL2 passes the Windows GPU | `FAIL` on WSL when it is missing: the Windows driver needs WSL support |
| P3.2 | `libd3d12.so`, `libd3d12core.so` and `libdxcore.so` in `/usr/lib/wsl/lib`, where WSL mounts the Windows driver's libraries | `WARN` when they are missing |
| P3.3 | the same libraries linked into `/run/opengl-driver/lib`, which is what `wsl.useWindowsDriver = true` does on NixOS-WSL | `WARN` with the options when they are not |
| P3.4 | the OpenGL renderer that `eglinfo -B` and `glxinfo -B` report on the machine, in four environments: as it is, with `LD_LIBRARY_PATH=/usr/lib/wsl/lib`, with `GALLIUM_DRIVER=d3d12`, and with both | `PASS` when one of them reports a hardware renderer (`d3d12` or other), naming the environment that does; `WARN` when all report `llvmpipe` or none works |
| P3.5 | `qemu -device help` and `-display help` list `virtio-vga-gl` (`virtio-gpu-gl-pci` on aarch64) and `gtk` | `FAIL` when not: the QEMU of nixpkgs lacks it |
| P3.6 | `-display gtk,gl=on -device virtio-vga-gl` for `--hold` seconds | the same as P2.2, with the environment of P3.4's variant tried when the default one fails; QEMU's refusal is printed |
| P3.7 | your answer to "did the VirGL window appear too?" | as P2.3 |

The facts include the contents of `/usr/lib/wsl/lib` and `/usr/lib/wsl/drivers`,
the Windows GPU name and driver version from `Win32_VideoController`, and each
renderer line per variant (`host_gl[default]`, `host_gl[d3d12]`, ...), which names
the renderer that each platform of `eglinfo` (X11, Wayland, GBM, surfaceless)
reports. A renderer string that names `d3d12` is Mesa's Direct3D 12 driver,
which draws on the Windows GPU; `llvmpipe` is Mesa's software rasterizer.

What happens inside the guest is not checked: the guest of this probe has no
graphics stack, so the renderer that a Plasma session in it gets (VirGL on the
host's renderer, or `llvmpipe`) is for the real guest to report.

### P4: the Windows side

| Check | What it looks at | Verdict |
|---|---|---|
| P4.1 | `cmd.exe /c ver` through interop | `FAIL` with the NixOS-WSL interop options when Windows programs do not run from WSL |
| P4.2 | `mstsc.exe`, Windows' RDP client, on `PATH` or in `System32` | `PASS` when found; `WARN` with the `includePath` option when not |
| P4.3 | two `socat` listeners on free ports above 20000, one on `127.0.0.1` and one on `0.0.0.0`, and a TCP connection to each from `powershell.exe`, as `127.0.0.1` and as `localhost` | `PASS` when Windows reaches the loopback listener both ways; `WARN` when only `127.0.0.1` works, or only the `0.0.0.0` listener is reachable; `FAIL` when nothing is, with the `.wslconfig` lines for `localhostForwarding` and `networkingMode` |

The facts are the path of `cmdkey.exe` (which a later step would use to hand
Windows the RDP credentials), the networking mode (`wslinfo --networking-mode`,
or `networkingMode` of `.wslconfig`: NAT by default, or mirrored), the
`localhostForwarding` setting, the WSL machine's addresses, and the four
connection results.

Outside WSL, P4 says so and checks nothing.

### P5: resources

The real guest wants 8 GiB of memory, 4 or more cores, and about 25 GiB of disk,
plus the space the Nix build takes.

| Check | What it looks at | Verdict |
|---|---|---|
| P5.1 | `MemTotal` of the WSL machine | `PASS` from 10 GiB (8 for the guest, 2 for WSL and Nix next to it); `WARN` from 6 GiB, where the guest would run with `--memory 6144`; `FAIL` below, each with the `memory=` and `swap=` lines for `.wslconfig` |
| P5.2 | `nproc` | `PASS` from 4; `WARN` below, with the `processors=` line |
| P5.3 | the free space under `/nix`, or on the Windows drive under the virtual disk when that is less | `PASS` from 40 GiB (25 for the closure and the guest's disk, 10 for the build); `WARN` from 25; `FAIL` below |

On WSL the virtual disk looks like a terabyte; what limits it is the Windows
drive it lives on, so the probe reads `/mnt/c` as well and takes the smaller of
the two.

### P6: virtiofs

The real guest does not copy the host's `/nix/store`: it mounts it. On a Linux
host nixpkgs' QEMU runner starts one `virtiofsd` per shared directory (the
store, the repository, the directory of the credentials), and QEMU joins the
guest's virtio-fs device to each daemon's socket, with the guest's memory in a
shared `memfd`. This probe builds the same chain, with the same options as the
runner, for the host's `/nix/store` (read-only) and a temporary directory
(writable), and a guest whose kernel is the one the real guest boots.

| Check | What it looks at | Verdict |
|---|---|---|
| P6.1 | `/dev/fuse` exists and the user can open it read-write | `PASS`; `WARN` with the NixOS lines when it is missing or closed: `virtiofsd` does not open it (see below), so the guest can still start |
| P6.2 | `virtiofsd` (the one of nixpkgs, as the runner uses) starts as the user, with `--sandbox=none --seccomp=none --translate-uid ...`, and makes its socket, for `/nix/store` read-only and a temporary directory writable | `FAIL` with the daemon's output when a socket does not appear; `SKIP` when the program is not there |
| P6.3 | QEMU boots the guest with `-object memory-backend-memfd,share=on`, `-machine memory-backend=` and two `vhost-user-fs-pci` devices, under KVM when `/dev/kvm` opens and under TCG otherwise | `FAIL` with QEMU's error, or when the guest does not run to its end |
| P6.4 | the guest loads `virtio_pci` and `virtiofs`, mounts `/nix/store` read-only, and its checksum of the busybox it came with equals the host's; a write is refused | `FAIL` with the guest's error and, when the module did not load, that; `WARN` when the share could be written |
| P6.5 | the guest mounts the writable directory, reads a random token the host wrote, and writes a file that the host reads | `FAIL` when any of the three does not work |

The facts are the path and version of `virtiofsd`, the warnings it printed, how
each module loaded in the guest (`built in` for a kernel that has the transport
in its image), the file systems the guest lists, what the guest saw (the
checksum, the owner and mode of the host user's file, of the file it wrote) and
the owner of the file it wrote as the host sees it.

What is known of `virtiofsd` before WSL2 (read at time of writing, 2026-10-28,
from `virtiofsd` 1.14.0 and nixpkgs' runner on Linux machines that are not WSL2):

- it runs as an ordinary user with the runner's options, and prints two
  warnings, that it could not raise the limit of open files above the hard limit,
  and that it cannot use file handles for the root node (`Operation not
  permitted`) and falls back to `O_PATH` descriptors; both are harmless;
- it never opens `/dev/fuse`: a `strace` of its start shows no such open. It
  speaks FUSE to the guest's kernel through the vhost-user socket, so the
  daemon needs the socket, a shared `memfd` in QEMU, and the directory;
- a file that the host user owns appears in the guest owned by the same numeric
  uid and gid (`--translate-uid host:65534:0:1` maps only the host's 65534),
  which is why the guest user of the demo cannot read a directory of a host
  user whose uid is not 1000 when its mode is 0700;
- the kernel configuration of WSL2 (`linux-msft-wsl-6.6.y`, `config-wsl` for
  `x86_64`, read from its repository, not measured on a machine) has
  `CONFIG_FUSE_FS=y`, `CONFIG_MEMFD_CREATE=y`, `CONFIG_USER_NS=y`,
  `CONFIG_UNIX=y`, `CONFIG_DEVTMPFS=y` and `CONFIG_KVM_AMD=m`: nothing the
  chain needs is missing from it. `/dev/fuse` is a device node that the kernel's
  devtmpfs makes closed to ordinary users, and udev's default rules open it, so
  where udev's rules do not apply it can be closed, which `virtiofsd` does not
  mind.

On WSL2 itself the chain is not measured at time of writing: the paste of the
first run is the first fact.

## The NixOS-WSL option names

The fixes name these options of NixOS-WSL: `wsl.useWindowsDriver`,
`wsl.interop.includePath`, `wsl.interop.register` and `wsl.wslConf.interop.enabled`,
and of NixOS itself `users.users.<name>.extraGroups`, `nix.settings.trusted-users`
and `nix.settings.experimental-features`. NixOS-WSL renames options now and then;
when `nixos-rebuild` rejects one, its options page has the current name.

## How the script is checked

The script is `scripts/linux/wsl2-probe.sh` and the flake wraps it
([`nix/linux/dev/wsl2-probe.nix`](../../../nix/linux/dev/wsl2-probe.nix)): `packages.<system>.wsl2-probe`
and the app of the same name, and `wsl2-probe-guest`, the kernel and the initrd
(with the modules of virtio's PCI transport and of virtiofs).
The initrd's init and the loop are `nix/linux/dev/wsl2-probe-init.sh` and
`nix/linux/dev/wsl2-probe-loop.sh`; the same loop file is what the script times on the
machine.

Two checks of the flake keep the script from rotting between the times someone
runs it. `wsl2-probe` builds the wrapper, which runs ShellCheck over the script
and over the guest's init, so neither can have a warning. `wsl2-probe-dry-run`
runs `wsl2-probe --dry-run` in the build sandbox, which has no KVM, no network
and no display, and expects a line for every probe, the paste block, and a
refusal of an unknown option. `just lint` runs ShellCheck on the script as well.

The script was exercised, at time of writing (2026-10-28), on machines that
are not WSL2:

- an `x86_64` NixOS machine with KVM: P1 passes with the ratio above; P2 passes
  under `xvfb-run` and reports "no display" without one; P3 reports the missing
  OpenGL driver and QEMU's refusal of `gl=on`;
- an `aarch64` NixOS machine without KVM: P1 reports the missing `/dev/kvm` and
  boots the guest under TCG alone;
- the `x86_64` machine again with stand-ins for Windows' `cmd.exe`,
  `powershell.exe` (PowerShell 7, running the script's PowerShell), `wsl.exe`
  and `wslpath` on `PATH` and `WSL_DISTRO_NAME` set, for the paths that need
  interop, and with `/dev/kvm`, `/proc/cpuinfo` and `/proc/meminfo` replaced
  inside a user namespace for the failing and short-of-resources verdicts;
- P6 on the `x86_64` machine with KVM (the whole chain passes; the guest boots
  in 1.2 s), on the `aarch64` machine under TCG (passes; 3.7 s), under TCG on
  the `x86_64` machine with `/dev/kvm` closed in a user namespace (passes;
  4 s), with `/dev/fuse` closed in a user namespace (`WARN`, and the rest
  passes), with `virtiofsd` off `PATH` (`SKIP`), with a `virtiofsd` that exits
  (`FAIL` with its output), and with a stand-in for QEMU that refuses the shared
  memory, or prints what a guest would after each way the mounts can fail (the
  verdicts of P6.3 to P6.5 for each).

Nothing in the script has run on WSL2 itself at time of writing, so the paste of
its first run there is the first measurement of any of this on WSL2.
