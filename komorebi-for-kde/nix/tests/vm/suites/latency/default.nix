{
  tier = "slow";
  kind = "headless";
  timeoutMinutes = 15;
  node =
    { pkgs, ... }:
    {
      environment.systemPackages = [ pkgs.dbus ];
      # this suite times the daemon, so it runs at the default log level, as a user's does
      services.komorebi.logLevel = "info";
    };
}
