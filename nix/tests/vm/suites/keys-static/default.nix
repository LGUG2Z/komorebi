{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 10;
  node =
    { config, pkgs, ... }:
    {
      # static shortcuts work with no komorebi process, and this test runs none
      services.komorebi.enable = false;

      # The shortcut files of keys.skhdrc, as `komorebic hotkeys compile` writes them, in the system
      # profile, where KWin finds them when the session starts
      environment.systemPackages = [
        config.services.komorebi.package
        (pkgs.runCommand "komorebi-keys-static" { } ''
          ${config.services.komorebi.package}/bin/komorebic hotkeys compile --mode auto \
            ${./keys.skhdrc} --output $out/share/kglobalaccel
        '')
      ];
    };
}
