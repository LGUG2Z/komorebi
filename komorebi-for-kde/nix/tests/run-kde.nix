# What `run-kde` says and starts, for machines that a test makes up from the variables the script
# reads (RUN_KDE_*, WAYLAND_DISPLAY, DISPLAY): a WSL 2 with WSLg and a GPU, one with WSLg and a
# software OpenGL or none, a Linux machine with no window, and machines that cannot run the guest.
# `--dry-run` prints what the script would start, which is the environment and the arguments the
# runner is given; `--doctor` prints what the machine lacks and how to fix it, and starts the
# virtiofsd of nixpkgs for real, as the sandbox's user, to see that it serves a directory. The
# sandbox has no KVM, no GPU and no WSL, which is the point: the script's answer does not depend on
# the machine it is run on. The expected texts are in `run-kde/`.
{ pkgs, runKde }:
let
  exe = pkgs.lib.getExe runKde;

  # The files that stand for a machine, and a command that runs the script in an environment of its
  # own, with the variables that follow it. @T@ is where the files are, which the expected texts do
  # not keep.
  setup = ''
    t=$PWD/machine
    mkdir -p $t/bin $t/repo $t/dri $t/wsl-lib $t/va
    touch $t/repo/flake.nix $t/kvm $t/kvm-denied $t/fuse $t/dxg $t/va/d3d12_drv_video.so
    chmod 000 $t/kvm-denied
    printf '#!/bin/sh\n' > $t/bin/nix
    printf '#!/bin/sh\necho "[ERROR virtiofsd] Error creating the vhost-user socket: Address in use (os error 98)" >&2\nexit 1\n' > $t/bin/virtiofsd-broken
    chmod +x $t/bin/nix $t/bin/virtiofsd-broken
    printf 'MemTotal:       16777216 kB\n' > $t/meminfo
    printf 'MemTotal:        6291456 kB\n' > $t/meminfo-small
    printf '5.15.167.4-microsoft-standard-WSL2\n' > $t/osrelease-wsl
    printf '6.18.0\n' > $t/osrelease-linux
    printf 'flags : fpu vme svm sse\n' > $t/cpuinfo-svm
    printf 'flags : fpu vme sse\n' > $t/cpuinfo-none

    machine() {
      env -i PATH=$t/bin:$PATH HOME=/home/user XDG_STATE_HOME=/state \
        KOMOREBI_REPO=$t/repo RUN_KDE_GUEST=run-kde-x86_64 \
        RUN_KDE_RUNNER_BIN=/nix/store/fake-nixos-vm/bin/run-komorebi-demo-vm \
        RUN_KDE_CPUINFO=$t/cpuinfo-svm RUN_KDE_STORE_DIR=$t RUN_KDE_FREE_GB=100 \
        RUN_KDE_DRI_DIR=$t/dri RUN_KDE_CPUS=12 RUN_KDE_FUSE_DEVICE=$t/fuse \
        RUN_KDE_WSL_LIB_DIR=$t/wsl-lib RUN_KDE_VA_DRIVERS_DIR=$t/va \
        "$@"
    }
    plain() { sed "s|$t|@T@|g"; }
  '';

  # a WSL 2 with WSLg, 16 GB of memory and KVM; and a Linux machine with none of WSL
  wslg = "WAYLAND_DISPLAY=wayland-0 DISPLAY=:0 RUN_KDE_OSRELEASE=$t/osrelease-wsl RUN_KDE_KVM_DEVICE=$t/kvm RUN_KDE_MEMINFO=$t/meminfo";
  linux = "RUN_KDE_OSRELEASE=$t/osrelease-linux RUN_KDE_KVM_DEVICE=$t/kvm RUN_KDE_MEMINFO=$t/meminfo RUN_KDE_HOST_RENDERER=";

  # runs `command`, which has to exit with `status` and print what `run-kde/NAME.expected` has
  scenario = name: status: command: ''
    echo "== ${name}"
    set +e
    ${command} > ${name}.raw 2>&1
    got=$?
    set -e
    plain < ${name}.raw > ${name}.out
    if [ "$got" != ${toString status} ]; then
      echo "${name}: the exit status is $got, not ${toString status}"
      cat ${name}.out
      exit 1
    fi
    diff -u ${./run-kde + "/${name}.expected"} ${name}.out
  '';

  check =
    name: scenarios:
    pkgs.runCommand name { nativeBuildInputs = [ pkgs.diffutils ]; } ''
      ${setup}
      ${builtins.concatStringsSep "\n" scenarios}
      touch $out
    '';
in
{
  run-kde-dry-run = check "run-kde-dry-run" [
    (scenario "dry-run-wslg-gpu" 0
      ''machine ${wslg} RUN_KDE_DXG_DEVICE=$t/dxg "RUN_KDE_HOST_RENDERER=D3D12 (AMD Radeon RX 7800 XT)" ${exe} --dry-run''
    )
    # without the d3d12 video driver the host's GPU still renders, and nothing says libva which driver
    (scenario "dry-run-wslg-gpu-no-video-driver" 0
      ''machine ${wslg} RUN_KDE_DXG_DEVICE=$t/dxg "RUN_KDE_HOST_RENDERER=D3D12 (AMD Radeon RX 7800 XT)" RUN_KDE_VA_DRIVERS_DIR=$t/absent ${exe} --dry-run''
    )
    # what a person has set is kept, and the directory of Windows' libraries comes first
    (scenario "dry-run-wslg-gpu-own-env" 0
      ''machine ${wslg} RUN_KDE_DXG_DEVICE=$t/dxg "RUN_KDE_HOST_RENDERER=D3D12 (AMD Radeon RX 7800 XT)" GALLIUM_DRIVER=zink LD_LIBRARY_PATH=/opt/lib LIBVA_DRIVER_NAME=radeonsi ${exe} --dry-run''
    )
    (scenario "dry-run-wslg-no-gpu" 0
      "machine ${wslg} RUN_KDE_DXG_DEVICE=$t/absent RUN_KDE_HOST_RENDERER= ${exe} --dry-run"
    )
    (scenario "dry-run-wslg-software-gl" 0
      ''machine ${wslg} RUN_KDE_DXG_DEVICE=$t/dxg "RUN_KDE_HOST_RENDERER=llvmpipe (LLVM 21.1.8, 256 bits)" ${exe} --dry-run''
    )
    (scenario "dry-run-headless" 0 "machine ${linux} RUN_KDE_MEMINFO=$t/meminfo-small ${exe} --dry-run")
    (scenario "dry-run-tcg" 0 "machine ${linux} RUN_KDE_KVM_DEVICE=$t/absent ${exe} --dry-run --tcg")
  ];

  run-kde-doctor = check "run-kde-doctor" [
    (scenario "doctor-wslg-gpu" 0
      ''machine ${wslg} RUN_KDE_DXG_DEVICE=$t/dxg "RUN_KDE_HOST_RENDERER=D3D12 (AMD Radeon RX 7800 XT)" RUN_KDE_WSLCONFIG=$t/none ${exe} --doctor''
    )
    (scenario "doctor-wslg-gpu-no-driver" 0
      ''machine ${wslg} RUN_KDE_DXG_DEVICE=$t/dxg "RUN_KDE_HOST_RENDERER=D3D12 (AMD Radeon RX 7800 XT)" RUN_KDE_WSL_LIB_DIR=$t/absent RUN_KDE_VA_DRIVERS_DIR=$t/absent RUN_KDE_WSLCONFIG=$t/none ${exe} --doctor''
    )
    (scenario "doctor-wsl-no-kvm" 1
      "machine WAYLAND_DISPLAY=w RUN_KDE_OSRELEASE=$t/osrelease-wsl RUN_KDE_KVM_DEVICE=$t/absent RUN_KDE_CPUINFO=$t/cpuinfo-none RUN_KDE_MEMINFO=$t/meminfo-small RUN_KDE_DXG_DEVICE=$t/absent RUN_KDE_HOST_RENDERER= RUN_KDE_WSLCONFIG=$t/none ${exe} --doctor"
    )
    (scenario "doctor-kvm-denied" 1 "machine ${linux} RUN_KDE_KVM_DEVICE=$t/kvm-denied ${exe} --doctor")
    (scenario "doctor-fuse-missing" 0 "machine ${linux} RUN_KDE_FUSE_DEVICE=$t/absent ${exe} --doctor")
    (scenario "doctor-fuse-denied" 0
      "machine ${linux} RUN_KDE_FUSE_DEVICE=$t/kvm-denied ${exe} --doctor"
    )
    (scenario "doctor-virtiofsd-broken" 1
      "machine ${linux} RUN_KDE_VIRTIOFSD=$t/bin/virtiofsd-broken ${exe} --doctor"
    )
  ];
}
