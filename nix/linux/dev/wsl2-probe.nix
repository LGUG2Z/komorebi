{
  pkgs,
  # the revision the probe says it came from; the checks pass a constant, so that they are the
  # same derivations for every commit
  rev,
}:

# scripts/wsl2-probe.sh, with the tools it uses from nixpkgs, and the tiny guest it boots.
#
# The guest is a kernel from the binary cache and an initrd made here of a static busybox, the
# kernel modules that virtiofs needs, and the init in wsl2-probe-init.sh: nothing in it is built
# from source, so the first run is a download. docs/dev/wsl2-probe.md says what the probe does.
let
  inherit (pkgs) lib;

  busybox = pkgs.pkgsStatic.busybox;

  # the kernel is the one the real guest boots (the default of NixOS), so that what the probe's
  # guest can do with virtiofs is what the real guest's kernel can do
  kernel = pkgs.linuxPackages.kernel;

  # the file of the kernel package that QEMU boots
  kernelImage = if pkgs.stdenv.hostPlatform.isAarch64 then "Image" else "bzImage";

  # virtio's PCI transport, FUSE and virtiofs are modules of that kernel, not built in; this is
  # the closure of them in the shape the NixOS initrd uses (modules.dep included, for modprobe)
  modules = pkgs.makeModulesClosure {
    kernel = pkgs.aggregateModules [ (lib.getOutput "modules" kernel) ];
    firmware = pkgs.runCommand "wsl2-probe-no-firmware" { } "mkdir -p $out";
    rootModules = [
      "virtio_pci"
      "virtiofs"
    ];
  };

  # the loop that the host and the guest both time, which busybox's ash runs in both
  loop = pkgs.writeTextFile {
    name = "wsl2-probe-loop.sh";
    text = builtins.readFile ./wsl2-probe-loop.sh;
    checkPhase = ''
      ${lib.getExe pkgs.shellcheck} --shell=sh "$target"
    '';
  };

  init = pkgs.writeTextFile {
    name = "wsl2-probe-init";
    executable = true;
    text = ''
      #!${busybox}/bin/sh
      PATH=${busybox}/bin
      # shellcheck source=/dev/null
      . ${loop}
      ${builtins.readFile ./wsl2-probe-init.sh}
    '';
    checkPhase = ''
      ${lib.getExe pkgs.shellcheck} --shell=sh "$target"
    '';
  };

  initrd = pkgs.makeInitrd {
    contents = [
      {
        object = init;
        symlink = "/init";
      }
      {
        object = "${modules}/lib/modules";
        symlink = "/lib/modules";
      }
    ];
  };

  guest = pkgs.runCommand "wsl2-probe-guest" { } ''
    mkdir -p $out
    ln -s ${kernel}/${kernelImage} $out/kernel
    ln -s ${initrd}/initrd $out/initrd
    ln -s ${busybox}/bin/busybox $out/busybox
    ln -s ${loop} $out/loop.sh
  '';

  probe = pkgs.writeShellApplication {
    name = "wsl2-probe";
    runtimeInputs = with pkgs; [
      # QEMU with GTK and VirGL, which the real guest uses, and the daemon that serves the guest's
      # /nix/store and shared directories
      qemu_kvm
      virtiofsd
      # eglinfo and glxinfo
      mesa-demos
      socat
      # what the script needs of a system, the same on every distribution
      coreutils
      findutils
      gawk
      gnugrep
      gnused
      iproute2
      jq
      procps
      util-linux
    ];
    # the script sets its own options: a probe that stops at the first failure answers nothing
    bashOptions = [ ];
    text = ''
      export WSL2_PROBE_GUEST=${guest}
      export WSL2_PROBE_REV=${rev}
      ${builtins.readFile ../../../scripts/linux/wsl2-probe.sh}
    '';
    meta = {
      description = "Probes a NixOS-WSL2 machine for KVM, WSLg, a GPU and RDP, for a Plasma guest";
      mainProgram = "wsl2-probe";
    };
  };
in
{
  inherit guest probe;
}
