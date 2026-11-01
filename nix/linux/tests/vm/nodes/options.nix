# The options the test nodes share.
{ lib, ... }:
{
  options.komorebi.vm.renderNode = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Give an x86_64 guest a virtio-gpu, so that KWin has a DRM render node. A node that draws sets
      this to false: the virtio-gpu would be a second display, KWin would draw on it, and a
      screenshot, which is of the other one, would show nothing. The test driver adds the one
      virtio-gpu an aarch64 guest has, whatever this says.
    '';
  };
}
