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

  kb = cfg.keybindings;

  # key bindings are configured when any of the three is set; the assertions say what is missing
  keybindingsSet = kb.format != null || kb.text != null || kb.file != null;
  # the file the daemon reads, named after its format so that it reads as one
  keybindingsFile =
    if kb.file != null then kb.file else pkgs.writeText "komorebi-${toString kb.format}rc" kb.text;

  # `komorebic hotkeys compile`: the static shortcuts of the file, in share/kglobalaccel, where KWin
  # finds them in the system profile, and what komorebic had to say about the file
  compiled = pkgs.runCommand "komorebi-keybindings" { } ''
    mkdir -p $out/share/kglobalaccel
    if ! ${lib.getExe' cfg.package "komorebic"} hotkeys compile \
        --format ${toString kb.format} --mode ${kb.mode} ${lib.optionalString kb.strict "--strict"} \
        ${keybindingsFile} --output $out/share/kglobalaccel 2> $out/diagnostics.txt; then
      cat $out/diagnostics.txt >&2
      exit 1
    fi
    cat $out/diagnostics.txt >&2
  '';

  keybindingsArgs = [
    "--keybindings"
    "${keybindingsFile}"
    "--keybindings-format"
    (toString kb.format)
    "--keybindings-mode"
    kb.mode
    "--chord-timeout"
    (toString kb.chordTimeout)
  ]
  ++ lib.concatMap (name: [
    "--app-alias"
    (lib.escapeShellArg "${name}=${lib.concatStringsSep "," kb.appAliases.${name}}")
  ]) (lib.attrNames kb.appAliases);

  # the KWin versions komorebi supports, and the KWin of this system
  supportedKwin = import ./supported-kwin.nix;
  kwinVersion = pkgs.kdePackages.kwin.version;
  kwinUntested = lib.versionOlder supportedKwin.testedMax kwinVersion;
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

    effects.borders = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Install the KWin effect that draws komorebi's borders, KDE Rounded Corners. This is
          installation only: whether borders are drawn is komorebi's setting (`komorebic border`),
          which asks KWin to load the effect when it is on. KWin finds effects when it starts, so
          an effect installed by a switch to a new system is found after the next login.

          Without the effect, komorebi draws no border and leaves no room for one.
        '';
      };

      package = lib.mkPackageOption pkgs "kde-rounded-corners-komorebi" {
        extraDescription = ''
          It must have `lib/qt-6/plugins/kwin/effects/plugins/kwin4_effect_shapecorners.so`, built
          against the KWin of the system and not loaded by default. The default is KDE Rounded
          Corners 0.11.0, which draws correctly on KWin 6.8.
        '';
      };
    };

    effects.animation = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Install the KWin effect that animates komorebi's windows going to their places, Geometry
          Change. This is installation only: whether windows are animated is komorebi's setting
          (`komorebic animation`), which asks KWin to load the effect when it is on. KWin finds
          effects when it starts, so an effect installed by a switch to a new system is found
          after the next login.

          Without the effect, windows jump to their places.
        '';
      };

      package = lib.mkPackageOption pkgs "kwin-effect-geometry-change-komorebi" {
        extraDescription = ''
          It must have `share/kwin/effects/kwin4_effect_geometry_change`, a scripted effect that
          is not loaded by default.
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
        `logLevel`, with `--config` and the checked `komorebi.json` when `configFile` or `config` is
        set, and with the key bindings when there are some. The module sets it, and setting it
        anywhere else is an error. A unit of your own that runs komorebi somewhere else, as the VM
        tests do in their headless sessions, starts it with this, the way the module does.
      '';
    };

    keybindings = {
      format = lib.mkOption {
        type = lib.types.nullOr (
          lib.types.enum [
            "skhd"
            "whkd"
            "sxhkd"
          ]
        );
        default = null;
        example = "skhd";
        description = ''
          The format of the key bindings: that of skhd's skhdrc, whkd's whkdrc or sxhkd's sxhkdrc.
          Required with `text` or `file`.
        '';
      };

      text = lib.mkOption {
        type = lib.types.nullOr lib.types.lines;
        default = null;
        example = ''
          alt - h : komorebic focus left
          alt - l : komorebic focus right
        '';
        description = ''
          The key bindings, written in `format`. Set this or `file`, not both. skhd's `.load`,
          which reads another file, is refused: the file would not be in the store.
        '';
      };

      file = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = lib.literalExpression "./skhdrc";
        description = "A file of key bindings, written in `format`. Set this or `text`, not both.";
      };

      mode = lib.mkOption {
        type = lib.types.enum [
          "auto"
          "engine"
          "static"
        ];
        default = "engine";
        description = ''
          Which bindings KDE runs by itself, as static shortcuts that the module installs in the
          system profile, and which the daemon registers and runs (the engine). `engine`: every
          binding is the engine's. `auto`: every binding that can be a static shortcut is one (one
          combo, pressed, the same in every application, in a file with no modes, blacklist or
          pause binding), and the engine runs the others. `static`: only static shortcuts; what
          needs the engine is left out, with a warning. A static shortcut works with no komorebi
          running and shows in System Settings, but takes about 0.2 s to run its command; the
          engine takes a few milliseconds.
        '';
      };

      chordTimeout = lib.mkOption {
        type = lib.types.ints.positive;
        default = 3000;
        description = "How long, in milliseconds, the engine waits for the next key of a chord chain.";
      };

      layout = lib.mkOption {
        type = lib.types.enum [ "us" ];
        default = "us";
        description = ''
          The keyboard layout positional key names are read with: skhd's macOS keycodes (`0x21`)
          and whkd's `oem_*` keys, and the symbol a shifted key types. `us` is the only one.
        '';
      };

      strict = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Fail the build when something in the key bindings is not realized the way the file's own
          hotkey daemon realizes it: a diagnostic of `komorebic hotkeys compile` that is a warning
          or an error. The diagnostics are in the build log and in `diagnostics.txt` of the
          `komorebi-keybindings` package either way.
        '';
      };

      appAliases = lib.mkOption {
        type = lib.types.attrsOf (lib.types.listOf lib.types.str);
        default = { };
        example = {
          firefox = [
            "firefox-esr"
            "librewolf"
          ];
        };
        description = ''
          Other names of an application, for the bindings that are for one application: an skhdrc
          or a whkdrc names applications as macOS or Windows does, and KDE knows them by their
          executable, app id or desktop entry.
        '';
      };
    };

    kwin.untestedIsError = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Refuse a KWin newer than the newest one komorebi has been tested with (`testedMax` in
        the repository's `nix/supported-kwin.nix`), instead of warning about it. A KWin older than
        the oldest one komorebi supports (`min`) is refused either way.
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
        ++ lib.optionals keybindingsSet keybindingsArgs
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
        {
          assertion = keybindingsSet -> (kb.text != null) != (kb.file != null);
          message = ''
            services.komorebi.keybindings needs exactly one of text and file: the key bindings
            themselves, or the file they are in.
          '';
        }
        {
          assertion = keybindingsSet -> kb.format != null;
          message = ''
            services.komorebi.keybindings needs a format: "skhd", "whkd" or "sxhkd".
          '';
        }
        {
          assertion =
            !(
              keybindingsSet
              && kb.format == "skhd"
              && lib.any (line: lib.hasPrefix ".load" (lib.trim line)) (
                lib.splitString "\n" (if kb.text != null then kb.text else builtins.readFile kb.file)
              )
            );
          message = ''
            services.komorebi.keybindings: skhd's .load is not supported. It reads another file,
            which would not be in the store; put its bindings in this one.
          '';
        }
        {
          assertion = lib.versionAtLeast kwinVersion supportedKwin.min;
          message = ''
            services.komorebi needs KWin ${supportedKwin.min} or newer, and pkgs.kdePackages.kwin is
            ${kwinVersion}. Use a nixpkgs with Plasma ${supportedKwin.min} or newer.
          '';
        }
        {
          assertion = !(cfg.kwin.untestedIsError && kwinUntested);
          message = ''
            pkgs.kdePackages.kwin is ${kwinVersion}, and komorebi has been tested with KWin
            ${supportedKwin.testedMax} and older. services.komorebi.kwin.untestedIsError is true, so
            this is an error; set it to false to build the system with a warning instead.
          '';
        }
      ];

      warnings =
        lib.optional (
          cfg.configFile != null && cfg.config != null
        ) "services.komorebi.config is ignored: services.komorebi.configFile is set."
        ++
          lib.optional (cfg.configFile != null && cfg.applicationSpecificConfiguration.sources != [ ])
            "services.komorebi.applicationSpecificConfiguration is ignored: services.komorebi.configFile is set."
        ++
          lib.optional
            (
              cfg.configFile == null
              && cfg.config != null
              && (cfg.config.border or null) == true
              && !cfg.effects.borders.enable
            )
            "services.komorebi.config sets border to true, but services.komorebi.effects.borders.enable is false: without the effect, komorebi draws no border."
        ++ lib.optional (kwinUntested && !cfg.kwin.untestedIsError) ''
          pkgs.kdePackages.kwin is ${kwinVersion}, and komorebi has been tested with KWin
          ${supportedKwin.testedMax} and older. Set services.komorebi.kwin.untestedIsError = true
          to make this an error.
        '';

      environment.systemPackages = [
        cfg.package
      ]
      ++ lib.optional cfg.effects.borders.enable cfg.effects.borders.package
      ++ lib.optional cfg.effects.animation.enable cfg.effects.animation.package
      ++ lib.optional keybindingsSet compiled;

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
          # kglobalaccel keeps the keys of a daemon that died until they are unregistered: without
          # this they would stay swallowed, running nothing, until the next start
          ExecStopPost = "${lib.getExe' cfg.package "komorebic"} hotkeys release";
          Restart = "on-failure";
          # what komorebi exits with when it is stopped with SIGTERM, as systemd stops a unit
          SuccessExitStatus = 130;
        };
      };
    })
  ];
}
