# `services.komorebi`: komorebi in the Plasma session of every user of a NixOS machine. The flake
# exports this module as `nixosModules.default` and `nixosModules.komorebi`, together with its
# overlay, which provides `pkgs.komorebi-full` (see docs/installation.md). What the module
# guarantees, and what enforces each guarantee, is in docs/dev/ARCHITECTURE.md; its tests are
# nix/tests/nixos-module.nix and the VM suite `module`.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.komorebi;
in
{
  options.services.komorebi = {
    enable = lib.mkEnableOption "komorebi, the tiling window manager, in the Plasma session of every user";

    package = lib.mkPackageOption pkgs "komorebi-full" {
      extraDescription = ''
        It must have `bin/komorebi`, which the unit runs, and `bin/komorebic`. The module installs
        both in the system profile.
      '';
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      default = "info";
      description = ''
        How much the daemon logs: its `--log-level`. The log goes to the journal of the unit
        (`journalctl --user -u komorebi`) and to the daemon's log files, which `komorebic log`
        follows.
      '';
    };

    execStart = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      description = ''
        The command line the unit starts komorebi with: `bin/komorebi` of `package`, with
        `logLevel`. The module sets it, and setting it anywhere else is an error. A unit of your
        own that runs komorebi somewhere else, as the VM tests do in their headless sessions,
        starts it with this, the way the module does.
      '';
    };
  };

  config = lib.mkMerge [
    {
      services.komorebi.execStart = "${lib.getExe' cfg.package "komorebi"} --log-level ${cfg.logLevel}";
    }

    (lib.mkIf cfg.enable {
      assertions = [
        {
          assertion = config.services.desktopManager.plasma6.enable;
          message = ''
            services.komorebi needs Plasma 6: komorebi runs in a Plasma Wayland session, and the
            session starts it. Set services.desktopManager.plasma6.enable = true.
          '';
        }
      ];

      environment.systemPackages = [ cfg.package ];

      # A user unit, so it runs for every user who logs in to Plasma. plasma-workspace.target is
      # the target of a Plasma session's services, and plasma-core.target, which it requires, is
      # ordered after KWin's service: the daemon exits when KWin is not on the session bus.
      systemd.user.services.komorebi = {
        description = "komorebi, the tiling window manager";
        wantedBy = [ "plasma-workspace.target" ];
        after = [ "plasma-core.target" ];
        # NixOS gives a unit a PATH made of a few store paths; this is the PATH a terminal in the
        # user's session has
        enableDefaultPath = false;
        environment.PATH = lib.concatStringsSep ":" [
          "/run/wrappers/bin"
          "/etc/profiles/per-user/%u/bin"
          "/run/current-system/sw/bin"
        ];
        serviceConfig = {
          ExecStart = cfg.execStart;
          Restart = "on-failure";
          # what komorebi exits with when it is stopped with SIGTERM, as systemd stops a unit
          SuccessExitStatus = 130;
        };
      };
    })
  ];
}
