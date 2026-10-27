# `nixos-module-eval`: `services.komorebi` in real NixOS systems, evaluated and never built. Each
# scenario is a `nixosSystem` with the module, and the checks read what it evaluates to: the
# assertions, the unit the module defines, and its read-only `execStart`. A scenario that must
# not evaluate is caught with `builtins.tryEval`. Nothing here builds a Linux package (the
# derivation below gets only check names and booleans), so a Mac runs it too.
{
  pkgs,
  # nixpkgs' `lib.nixosSystem`
  nixosSystem,
  # the module, as the flake exports it, with its overlay
  module,
  # the flake's komorebi-full for a system, which the overlay has to provide
  komorebiFor,
  # the flake's KDE Rounded Corners for a system, which the overlay has to provide as well
  borderEffectFor,
  # the flake's Geometry Change for a system, likewise
  animationEffectFor,
}:
let
  inherit (pkgs) lib;

  # the systems are of the host's architecture
  system = "${pkgs.stdenv.hostPlatform.parsed.cpu.name}-linux";

  # enough of a machine for every NixOS assertion to hold, since no scenario is ever built
  machine = {
    nixpkgs.hostPlatform = system;
    boot.loader.grub.enable = false;
    fileSystems."/" = {
      device = "/dev/vda";
      fsType = "ext4";
    };
    system.stateVersion = lib.trivial.release;
  };

  plasma = {
    services.desktopManager.plasma6.enable = true;
  };

  eval =
    scenario:
    (nixosSystem {
      modules = [
        module
        machine
        scenario
      ];
    }).config;

  enabled = eval {
    imports = [ plasma ];
    services.komorebi.enable = true;
  };

  disabled = eval plasma;

  withoutPlasma = eval { services.komorebi.enable = true; };

  debug = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      logLevel = "debug";
    };
  };

  # a package that is not the overlay's: any package will do, since nothing is built
  otherPackage = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        package = pkgs.hello;
      };
    }
  );

  withConfigFile = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        configFile = pkgs.writeText "komorebi.json" "{}";
      };
    }
  );

  withConfig = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      config = {
        default_workspace_padding = 20;
        window_container_behaviour = "Append";
        float_override_placement = "Center";
        monocle_focus_behaviour = "Cycle";
        monitors = [
          {
            floating_layer_behaviour = "Tile";
            workspaces = [
              {
                name = "web";
                layout = "BSP";
              }
              {
                name = "code";
                layout = "Columns";
                window_container_behaviour_rules."2" = "Append";
                float_override = false;
                floating_layer_behaviour = "Float";
              }
            ];
          }
        ];
      };
    };
  };

  # a layout komorebi does not have
  badLayout = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      config.monitors = [
        {
          workspaces = [
            {
              name = "x";
              layout = "Nope";
            }
          ];
        }
      ];
    };
  };

  withSource = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        config.resize_delta = 70;
        applicationSpecificConfiguration.sources = [
          {
            owner = "someone";
            repo = "komorebi-rules";
            rev = "0123456789abcdef0123456789abcdef01234567";
            hash = pkgs.lib.fakeHash;
          }
        ];
      };
    }
  );

  # the same source and config, with the sources turned off
  withSourceOff = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        config.resize_delta = 70;
        applicationSpecificConfiguration = {
          enable = false;
          sources = [
            {
              owner = "someone";
              repo = "komorebi-rules";
              rev = "0123456789abcdef0123456789abcdef01234567";
              hash = pkgs.lib.fakeHash;
            }
          ];
        };
      };
    }
  );

  # the sources turned off, and no config: nothing is left to write
  onlySourceOff = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        applicationSpecificConfiguration = {
          enable = false;
          sources = [
            {
              owner = "someone";
              repo = "komorebi-rules";
              rev = "0123456789abcdef0123456789abcdef01234567";
              hash = pkgs.lib.fakeHash;
            }
          ];
        };
      };
    }
  );

  bothConfigs = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        configFile = pkgs.writeText "komorebi.json" "{}";
        config.resize_delta = 70;
      };
    }
  );

  noBorderEffect = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      effects.borders.enable = false;
    };
  };

  # borders asked for in the configuration, and the effect that draws them left out
  borderWithoutEffect = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      config.border = true;
      effects.borders.enable = false;
    };
  };

  # the same, with the effect installed, and with borders turned off in the configuration
  borderWithEffect = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      config.border = true;
    };
  };

  noBorderWithoutEffect = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      config.border = false;
      effects.borders.enable = false;
    };
  };

  noAnimationEffect = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      effects.animation.enable = false;
    };
  };

  # a package that is not the overlay's: any package will do, since nothing is built
  otherBorderEffect = eval (
    { pkgs, ... }:
    {
      imports = [ plasma ];
      services.komorebi = {
        enable = true;
        effects.borders.package = pkgs.hello;
      };
    }
  );

  unknownLevel = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      logLevel = "verbose";
    };
  };

  execStartSet = eval {
    imports = [ plasma ];
    services.komorebi = {
      enable = true;
      execStart = "/bin/true";
    };
  };

  failed = config: map (a: a.message) (lib.filter (a: !a.assertion) config.assertions);
  unit = config: config.systemd.user.services.komorebi;
  evaluates = value: (builtins.tryEval value).success;

  checks = [
    {
      name = "enabled with Plasma 6, every assertion holds";
      ok = failed enabled == [ ];
      got = failed enabled;
    }
    {
      name = "the unit is started by plasma-workspace.target";
      ok = (unit enabled).wantedBy == [ "plasma-workspace.target" ];
      got = (unit enabled).wantedBy;
    }
    {
      name = "the unit starts after plasma-core.target";
      ok = lib.elem "plasma-core.target" (unit enabled).after;
      got = (unit enabled).after;
    }
    {
      name = "the unit is restarted when it fails";
      ok = (unit enabled).serviceConfig.Restart == "on-failure";
    }
    {
      name = "the unit counts the exit status of a SIGTERM (130) as success";
      ok = (unit enabled).serviceConfig.SuccessExitStatus == 130;
    }
    {
      name = "the unit runs execStart";
      ok = (unit enabled).serviceConfig.ExecStart == enabled.services.komorebi.execStart;
      got = (unit enabled).serviceConfig.ExecStart;
    }
    {
      name = "execStart is bin/komorebi of the package, at the info level";
      ok =
        enabled.services.komorebi.execStart
        == "${enabled.services.komorebi.package}/bin/komorebi --log-level info";
      got = enabled.services.komorebi.execStart;
    }
    {
      name = "the default package is the flake's komorebi-full, from the overlay";
      ok = enabled.services.komorebi.package.outPath == (komorebiFor system).outPath;
    }
    {
      name = "the unit has the PATH of the user's session";
      ok =
        (unit enabled).environment.PATH
        == "/run/wrappers/bin:/etc/profiles/per-user/%u/bin:/run/current-system/sw/bin";
      got = (unit enabled).environment.PATH;
    }
    {
      name = "the package is installed in the system profile";
      ok = lib.elem enabled.services.komorebi.package enabled.environment.systemPackages;
    }
    {
      name = "the default border effect is the flake's KDE Rounded Corners, from the overlay";
      ok = enabled.services.komorebi.effects.borders.package.outPath == (borderEffectFor system).outPath;
    }
    {
      name = "the border effect is installed in the system profile";
      ok = lib.elem enabled.services.komorebi.effects.borders.package enabled.environment.systemPackages;
    }
    {
      name = "with the border effect off, komorebi is installed and the effect is not";
      ok =
        lib.elem noBorderEffect.services.komorebi.package noBorderEffect.environment.systemPackages
        && !(lib.elem noBorderEffect.services.komorebi.effects.borders.package noBorderEffect.environment.systemPackages);
    }
    {
      name = "another border effect is the one installed";
      ok =
        lib.elem otherBorderEffect.services.komorebi.effects.borders.package otherBorderEffect.environment.systemPackages
        && !(lib.elem enabled.services.komorebi.effects.borders.package otherBorderEffect.environment.systemPackages);
    }
    {
      name = "the default animation effect is the flake's Geometry Change, from the overlay";
      ok =
        enabled.services.komorebi.effects.animation.package.outPath == (animationEffectFor system).outPath;
    }
    {
      name = "the animation effect is installed in the system profile";
      ok = lib.elem enabled.services.komorebi.effects.animation.package enabled.environment.systemPackages;
    }
    {
      name = "with the animation effect off, the border effect is installed and the animation effect is not";
      ok =
        lib.elem noAnimationEffect.services.komorebi.effects.borders.package noAnimationEffect.environment.systemPackages
        && !(lib.elem noAnimationEffect.services.komorebi.effects.animation.package noAnimationEffect.environment.systemPackages);
    }
    {
      name = "a configuration that draws borders, without the effect that draws them, is warned about";
      ok = lib.any (lib.hasInfix "effects.borders.enable is false") borderWithoutEffect.warnings;
      got = borderWithoutEffect.warnings;
    }
    {
      name = "borders with the effect, and no borders without it, are not warned about";
      ok =
        !(lib.any (lib.hasInfix "effects.borders") borderWithEffect.warnings)
        && !(lib.any (lib.hasInfix "effects.borders") noBorderWithoutEffect.warnings)
        && !(lib.any (lib.hasInfix "effects.borders") noBorderEffect.warnings);
      got = borderWithEffect.warnings ++ noBorderWithoutEffect.warnings ++ noBorderEffect.warnings;
    }
    {
      name = "disabled, every assertion holds";
      ok = failed disabled == [ ];
      got = failed disabled;
    }
    {
      name = "disabled, there is no unit";
      ok = !(disabled.systemd.user.services ? komorebi);
    }
    {
      name = "disabled, nothing is installed";
      ok =
        !(lib.elem disabled.services.komorebi.package disabled.environment.systemPackages)
        && !(lib.elem disabled.services.komorebi.effects.borders.package disabled.environment.systemPackages)
        && !(lib.elem disabled.services.komorebi.effects.animation.package disabled.environment.systemPackages);
    }
    {
      name = "without Plasma 6, an assertion fails and names the option to set";
      ok = lib.any (lib.hasInfix "services.desktopManager.plasma6.enable = true") (failed withoutPlasma);
      got = failed withoutPlasma;
    }
    {
      name = "logLevel reaches execStart and the unit";
      ok =
        debug.services.komorebi.execStart
        == "${debug.services.komorebi.package}/bin/komorebi --log-level debug"
        && (unit debug).serviceConfig.ExecStart == debug.services.komorebi.execStart;
      got = (unit debug).serviceConfig.ExecStart;
    }
    {
      name = "another package is the one the unit runs and the one installed";
      ok =
        (unit otherPackage).serviceConfig.ExecStart
        == "${otherPackage.services.komorebi.package}/bin/komorebi --log-level info"
        && lib.elem otherPackage.services.komorebi.package otherPackage.environment.systemPackages;
      got = (unit otherPackage).serviceConfig.ExecStart;
    }
    {
      name = "without a configFile, the unit passes no --config";
      ok = !(lib.hasInfix "--config" enabled.services.komorebi.execStart);
      got = enabled.services.komorebi.execStart;
    }
    {
      name = "configFile reaches execStart and the unit as --config, after the log level";
      ok =
        lib.hasPrefix "${withConfigFile.services.komorebi.package}/bin/komorebi --log-level info --config /nix/store/" withConfigFile.services.komorebi.execStart
        && (unit withConfigFile).serviceConfig.ExecStart == withConfigFile.services.komorebi.execStart;
      got = (unit withConfigFile).serviceConfig.ExecStart;
    }
    {
      name = "with neither a configFile nor a config, there is nothing to write";
      ok = enabled.services.komorebi.internal.configAttrs == null;
    }
    {
      name = "config is the komorebi.json the daemon is given, with no null left in it";
      ok =
        withConfig.services.komorebi.internal.configAttrs.default_workspace_padding == 20
        &&
          map (w: w.name) (lib.head withConfig.services.komorebi.internal.configAttrs.monitors).workspaces
          == [
            "web"
            "code"
          ]
        && !(lib.hasInfix "null" (builtins.toJSON withConfig.services.komorebi.internal.configAttrs));
      got = withConfig.services.komorebi.internal.configAttrs;
    }
    {
      name = "the keys of the stacks, the float override, monocle and the floating layer are typed options";
      ok =
        withConfig.services.komorebi.internal.configAttrs.window_container_behaviour == "Append"
        && withConfig.services.komorebi.internal.configAttrs.float_override_placement == "Center"
        && withConfig.services.komorebi.internal.configAttrs.monocle_focus_behaviour == "Cycle"
        &&
          (lib.head withConfig.services.komorebi.internal.configAttrs.monitors).floating_layer_behaviour
          == "Tile"
        &&
          (lib.elemAt (lib.head withConfig.services.komorebi.internal.configAttrs.monitors).workspaces 1)
          .window_container_behaviour_rules."2" == "Append";
      got = withConfig.services.komorebi.internal.configAttrs;
    }
    {
      name = "config reaches execStart as a komorebi.json that was checked against the schema";
      ok =
        lib.hasInfix " --config " withConfig.services.komorebi.execStart
        && lib.hasSuffix "-validated-komorebi.json" withConfig.services.komorebi.execStart
        && (unit withConfig).serviceConfig.ExecStart == withConfig.services.komorebi.execStart;
      got = withConfig.services.komorebi.execStart;
    }
    {
      name = "configFile is checked against the schema too";
      ok = lib.hasSuffix "-validated-komorebi.json" withConfigFile.services.komorebi.execStart;
      got = withConfigFile.services.komorebi.execStart;
    }
    {
      name = "a layout komorebi does not have does not evaluate";
      ok = !(evaluates (builtins.deepSeq badLayout.services.komorebi.internal.configAttrs true));
    }
    {
      name = "a source of application specific configuration is the daemon's app_specific_configuration_path";
      ok =
        lib.hasSuffix "-applications.json" withSource.services.komorebi.internal.configAttrs.app_specific_configuration_path
        && withSource.services.komorebi.internal.configAttrs.resize_delta == 70;
      got = withSource.services.komorebi.internal.configAttrs.app_specific_configuration_path;
    }
    {
      name = "with applicationSpecificConfiguration.enable = false, the sources are left out of config";
      ok =
        withSource.services.komorebi.internal.configAttrs ? app_specific_configuration_path
        && !(withSourceOff.services.komorebi.internal.configAttrs ? app_specific_configuration_path)
        && withSourceOff.services.komorebi.internal.configAttrs.resize_delta == 70;
      got = withSourceOff.services.komorebi.internal.configAttrs;
    }
    {
      name = "with the sources turned off and no config, there is nothing to write and no --config";
      ok =
        onlySourceOff.services.komorebi.internal.configAttrs == null
        && !(lib.hasInfix "--config" onlySourceOff.services.komorebi.execStart);
      got = onlySourceOff.services.komorebi.execStart;
    }
    {
      name = "with a configFile, config is ignored, and the module says so";
      ok =
        lib.any (lib.hasInfix "services.komorebi.config is ignored") bothConfigs.warnings
        && lib.hasSuffix "-validated-komorebi.json" bothConfigs.services.komorebi.execStart;
      got = bothConfigs.warnings;
    }
    {
      name = "a log level the daemon does not have does not evaluate";
      ok = !(evaluates unknownLevel.services.komorebi.logLevel);
    }
    {
      name = "execStart cannot be set";
      ok = !(evaluates execStartSet.services.komorebi.execStart);
    }
  ];

  failures = lib.filter (check: !check.ok) checks;

  # what a failed check got, without the store paths' context: this derivation must not depend on
  # a Linux package
  show =
    check:
    check.name
    + lib.optionalString (check ? got) (
      ": got " + builtins.unsafeDiscardStringContext (builtins.toJSON check.got)
    );
in
pkgs.runCommand "nixos-module-eval" { } (
  if failures == [ ] then
    ''
      ${lib.concatMapStringsSep "\n" (check: "echo ${lib.escapeShellArg "ok: ${check.name}"}") checks}
      echo "${toString (lib.length checks)} checks of services.komorebi passed"
      touch $out
    ''
  else
    ''
      ${lib.concatMapStringsSep "\n" (
        check: "echo ${lib.escapeShellArg "FAILED: ${show check}"}"
      ) failures}
      exit 1
    ''
)
