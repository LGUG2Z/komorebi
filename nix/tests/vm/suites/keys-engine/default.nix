{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 10;
  node =
    { config, ... }:
    {
      # the test starts, kills and stops its own daemon, with the key bindings it writes
      services.komorebi.enable = false;
      environment.systemPackages = [ config.services.komorebi.package ];
    };
}
