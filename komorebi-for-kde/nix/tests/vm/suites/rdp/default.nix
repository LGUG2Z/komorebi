{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 15;
  node =
    { pkgs, ... }:
    {
      # the RDP server of the session guest to develop in
      imports = [ ../../../../dev/krdp.nix ];
      komorebi.krdp.enable = true;

      # an X server of the client's own, and what reads its picture back
      environment.systemPackages = [
        pkgs.iproute2
        pkgs.netpbm
        pkgs.xvfb
        pkgs.xwd
      ];
    };
}
