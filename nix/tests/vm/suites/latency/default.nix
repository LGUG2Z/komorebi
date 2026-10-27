{
  tier = "slow";
  kind = "headless";
  timeoutMinutes = 15;
  node =
    { pkgs, ... }:
    {
      environment.systemPackages = [ pkgs.dbus ];
    };
}
