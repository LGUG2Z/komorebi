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
      ok = !(lib.elem disabled.services.komorebi.package disabled.environment.systemPackages);
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
