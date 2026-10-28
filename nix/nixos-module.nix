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

    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = lib.literalExpression "./komorebi.json";
      description = ''
        The `komorebi.json` the daemon is configured by. The unit starts komorebi with it, as
        `--config`, in place of the `komorebi.json` the daemon looks for in `~/.config/komorebi`
        (`$KOMOREBI_CONFIG_HOME`). The file is in the Nix store, so it does not change under the
        daemon: a new file is read when the daemon starts again. `komorebic configuration` still
        shows the file of the config home, which the daemon does not read while this is set. With
        no `configFile`, the daemon reads the file of the config home, if there is one.
      '';
    };

    execStart = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      description = ''
        The command line the unit starts komorebi with: `bin/komorebi` of `package`, with
        `logLevel`, and with `--config` and `configFile` when that is set. The module sets it,
        and setting it anywhere else is an error. A unit of your own that runs komorebi somewhere
        else, as the VM tests do in their headless sessions, starts it with this, the way the
        module does.
      '';
    };
  };

  config = lib.mkMerge [
    {
      services.komorebi.execStart = lib.concatStringsSep " " (
        [
          (lib.getExe' cfg.package "komorebi")
          "--log-level"
          cfg.logLevel
        ]
        ++ lib.optionals (cfg.configFile != null) [
          "--config"
          "${cfg.configFile}"
        ]
      );
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
