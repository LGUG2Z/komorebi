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

  jsonFormat = pkgs.formats.json { };

  # The options `just nixgen` makes from `schema.json`, which type `config`
  komorebiOptionsModule = import ./komorebi-options.nix { inherit lib; };

  # the Nix module system uses null for an optional field that is not set, and the schema wants
  # the field to be absent
  filterNulls =
    value:
    if value == null then
      null
    else if builtins.isList value then
      map filterNulls (builtins.filter (x: x != null) value)
    else if builtins.isAttrs value then
      lib.filterAttrs (_n: v: v != null) (builtins.mapAttrs (_n: v: filterNulls v) value)
    else
      value;

  # A komorebi.json, checked against the schema the Rust types make (`schema.json`). Comment
  # lines are taken out first: komorebi reads JSON with comments, and the schema checker does not.
  validateJsonFile =
    {
      name,
      jsonFile,
    }:
    pkgs.runCommand "validated-${name}" { nativeBuildInputs = [ pkgs.check-jsonschema ]; } ''
      grep -v '^\s*//' ${jsonFile} > clean.json
      check-jsonschema --schemafile ${../schema.json} clean.json
      cp clean.json $out
    '';

  fetchAppSpecificSource =
    source:
    pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/${source.owner}/${source.repo}/${source.rev}/${source.path}";
      inherit (source) hash;
    };

  applicationSpecificConfigFiles = map fetchAppSpecificSource cfg.applicationSpecificConfiguration.sources;

  # one file is a path, several are a list of paths
  appSpecificPaths =
    if builtins.length applicationSpecificConfigFiles == 1 then
      "${builtins.head applicationSpecificConfigFiles}"
    else
      map (f: "${f}") applicationSpecificConfigFiles;

  shouldUseAppSpecific =
    cfg.applicationSpecificConfiguration.enable && applicationSpecificConfigFiles != [ ];

  # What `config` and `applicationSpecificConfiguration` make: the komorebi.json as an attribute
  # set, with no null in it, or null when there is nothing to write
  configAttrs =
    if cfg.config == null && !shouldUseAppSpecific then
      null
    else
      filterNulls (
        (if cfg.config == null then { } else cfg.config)
        // lib.optionalAttrs shouldUseAppSpecific { app_specific_configuration_path = appSpecificPaths; }
      );

  # the file the daemon is given: the one `configFile` names, or the one the options make, each
  # checked against the schema
  komorebiConfigFile =
    if cfg.configFile != null then
      validateJsonFile {
        name = "komorebi.json";
        jsonFile = cfg.configFile;
      }
    else if configAttrs != null then
      validateJsonFile {
        name = "komorebi.json";
        jsonFile = jsonFormat.generate "komorebi.json" configAttrs;
      }
    else
      null;
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
        The `komorebi.json` the daemon is configured by, checked against the schema of
        `komorebi.json` when the system is built. The unit starts komorebi with it, as `--config`,
        in place of the `komorebi.json` the daemon looks for in `~/.config/komorebi`
        (`$KOMOREBI_CONFIG_HOME`). The file is in the Nix store, so it does not change under the
        daemon: a new file is read when the daemon starts again. `komorebic configuration` still
        shows the file of the config home, which the daemon does not read while this is set. With
        neither `configFile` nor `config`, the daemon reads the file of the config home, if there
        is one. Ignored by `config`, which is ignored when this is set.
      '';
    };

    config = lib.mkOption {
      type = lib.types.nullOr komorebiOptionsModule.options.komorebi.type;
      default = null;
      example = lib.literalExpression ''
        {
          default_workspace_padding = 15;
          monitors = [
            {
              workspaces = [
                { name = "web"; layout = "BSP"; }
                { name = "code"; layout = "Columns"; }
              ];
            }
          ];
        }
      '';
      description = ''
        The configuration of komorebi, written as Nix: every key of `komorebi.json`, typed from
        the schema, so a layout that does not exist or a value of the wrong kind does not
        evaluate. The module writes it as a `komorebi.json`, checks that against the schema, and
        starts komorebi with it as `--config`. Ignored if `configFile` is set. A key you leave
        out is the default the schema gives it, if it gives one, and the daemon's own otherwise.
      '';
    };

    applicationSpecificConfiguration = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether to use the sources of application specific configuration.";
      };

      sources = lib.mkOption {
        type = lib.types.listOf (
          lib.types.submodule {
            options = {
              owner = lib.mkOption {
                type = lib.types.str;
                description = "GitHub repository owner.";
                example = "LGUG2Z";
              };
              repo = lib.mkOption {
                type = lib.types.str;
                description = "GitHub repository name.";
                example = "komorebi-application-specific-configuration";
              };
              rev = lib.mkOption {
                type = lib.types.str;
                description = "Git revision (commit hash, tag, or branch).";
                example = "09ebdcd95780b168cecde2f2d49096a58e91365f";
              };
              path = lib.mkOption {
                type = lib.types.str;
                default = "applications.json";
                description = "Path to the JSON file within the repository.";
              };
              hash = lib.mkOption {
                type = lib.types.str;
                description = "SHA256 hash of the file, for reproducibility.";
                example = "sha256-gfYubJpVEU6vxdd/vqujQhQX8mdAEE9ImugqdIyTQKk=";
              };
            };
          }
        );
        default = [ ];
        description = ''
          Files of application specific configuration, each fetched from GitHub, which the module
          gives the daemon as `app_specific_configuration_path` of the configuration it writes
          (it replaces the value `config` has for that key). There are none by default: no
          community file for KDE Plasma is known. Ignored when `configFile` is set.
        '';
        example = lib.literalExpression ''
          [
            {
              owner = "myuser";
              repo = "my-komorebi-rules";
              rev = "0123456789abcdef0123456789abcdef01234567";
              path = "applications.json";
              hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
            }
          ]
        '';
      };
    };

    internal.configAttrs = lib.mkOption {
      type = lib.types.nullOr lib.types.attrs;
      readOnly = true;
      internal = true;
      description = ''
        What `config` and `applicationSpecificConfiguration` make, before it is written as JSON:
        the `komorebi.json` as an attribute set with no null in it, or null when neither is set.
        The tests of the module read it, so that they need not build the file.
      '';
    };

    execStart = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      description = ''
        The command line the unit starts komorebi with: `bin/komorebi` of `package`, with
        `logLevel`, and with `--config` and the checked `komorebi.json` when `configFile` or
        `config` is set. The module sets it, and setting it anywhere else is an error. A unit of your own that runs komorebi somewhere
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
        ++ lib.optionals (komorebiConfigFile != null) [
          "--config"
          "${komorebiConfigFile}"
        ]
      );
      services.komorebi.internal.configAttrs = configAttrs;
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

      warnings =
        lib.optional (
          cfg.configFile != null && cfg.config != null
        ) "services.komorebi.config is ignored: services.komorebi.configFile is set."
        ++
          lib.optional (cfg.configFile != null && cfg.applicationSpecificConfiguration.sources != [ ])
            "services.komorebi.applicationSpecificConfiguration is ignored: services.komorebi.configFile is set.";

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
