{ lib, pkgs, ... }:

# The Plasma session guest as a plain disk image, for a hypervisor with a GPU: UTM on Apple Silicon
# runs QEMU with virtio-gpu-gl (virgl over ANGLE/CoreGL), which the QEMU in nixpkgs, and so the
# 9p-booted `plasma-vm-session`, cannot: that one composites with llvmpipe on the CPU.
#
# This is the same session as plasma-vm.nix (komorebi from the module, KRDP), booted from its own
# disk by UEFI instead of from a kernel and the host's /nix/store. docs/dev/utm.md says how to
# install it onto a disk image and point UTM at it.
{
  imports = [ ./plasma-vm.nix ];

  komorebi.plasmaVm.session = true;

  networking.hostName = lib.mkOverride 40 "plasma-utm";

  boot.loader = {
    grub = {
      enable = true;
      efiSupport = true;
      efiInstallAsRemovable = true;
      device = "nodev";
    };
    efi.canTouchEfiVariables = false;
    timeout = 1;
  };

  boot.initrd.availableKernelModules = [
    "virtio_pci"
    "virtio_blk"
    "virtio_net"
    "virtio_scsi"
    "xhci_pci"
    "usbhid"
  ];
  # kms from the start, so that sddm has a screen to start on
  boot.initrd.kernelModules = [ "virtio_gpu" ];
  boot.kernelParams = [ "console=tty0" ];
  boot.growPartition = true;

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
    autoResize = true;
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/ESP";
    fsType = "vfat";
  };

  # the window follows the guest's resolution, and the clipboard is shared
  services.spice-vdagentd.enable = true;
  services.qemuGuest.enable = true;

  # to check that the session really is on the GPU: `eglinfo | grep -i renderer` says virgl
  environment.systemPackages = [ pkgs.mesa-demos ];
}
