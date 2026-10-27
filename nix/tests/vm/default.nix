# The NixOS VM tests. Every directory under `suites/` is one test, `checks.<system>.vm-<name>`
# (Linux) and `legacyPackages.<system>.vm.<name>` (everywhere), so a new suite needs no edit to
# the flake. See docs/dev/vm-tests.md.
#
# `hostPkgs` is the pkgs of the machine that runs QEMU; the guests are always Linux. `komorebiFor`
# returns the komorebi package of a system, and the guests get the one of their own. `scale` says
# how much slower than KVM that machine is (1 for KVM, 1.5 for HVF on a Mac, 8 for TCG), and every
# wait in the suites is multiplied by it.
{
  hostPkgs,
  komorebiFor,
  scale ? 1,
}:
let
  inherit (hostPkgs) lib;

  # a Linux guest of the host's architecture, which is what the test driver boots
  guestSystem = "${hostPkgs.stdenv.hostPlatform.parsed.cpu.name}-linux";

  # the library the suites import, built for the python of the test driver
  komorebiVm =
    ps:
    ps.buildPythonPackage {
      pname = "komorebi-vm";
      version = "0.1.0";
      pyproject = true;
      src = lib.fileset.toSource {
        root = ./lib;
        fileset = ./lib;
      };
      build-system = [ ps.setuptools ];
      dependencies = [ ps.pillow ];
      doCheck = false;
    };

  # unit tests of the library against a fake machine, and the linters over it
  libCheck =
    let
      python = hostPkgs.python3.withPackages (ps: [
        ps.pytest
        (komorebiVm ps)
      ]);
    in
    hostPkgs.runCommand "vm-lib"
      {
        nativeBuildInputs = [
          python
          hostPkgs.ruff
          hostPkgs.ty
        ];
        src = lib.fileset.toSource {
          root = ./lib;
          fileset = ./lib;
        };
      }
      ''
        cp -r $src lib && chmod -R u+w lib && cd lib
        ruff check .
        ruff format --check .
        ty check --python ${python} .
        pytest -q -p no:cacheprovider
        touch $out
      '';

  suiteNames = lib.attrNames (
    lib.filterAttrs (_: type: type == "directory") (builtins.readDir ./suites)
  );

  suiteDefs = lib.genAttrs suiteNames (name: import (./suites + "/${name}"));

  mkSuite =
    name:
    let
      dir = ./suites + "/${name}";
      suite = suiteDefs.${name};
    in
    hostPkgs.testers.runNixOSTest {
      name = "komorebi-${name}";
      extraPythonPackages = ps: [ (komorebiVm ps) ];
      globalTimeout = builtins.ceil (suite.timeoutMinutes * 60 * scale);
      passthru.tier = suite.tier;

      # the nodes take the daemon as an argument, `komorebi`
      node.specialArgs.komorebi = komorebiFor guestSystem;

      nodes.machine = {
        imports = [ (./nodes + "/${suite.kind}.nix") ];
      };

      testScript = ''
        SCALE = ${toString scale}
      ''
      + builtins.readFile (dir + "/${suite.script or "test.py"}");
    };

  suites = lib.genAttrs suiteNames mkSuite;
in
{
  inherit suites;
  lib = libCheck;

  # the suites of each tier, for the CI matrix: { fast = [ "kwin" ... ]; slow = [ ... ]; }
  tiers = lib.groupBy (name: suiteDefs.${name}.tier) suiteNames;

  # the guests of every suite, so that a Mac can build them on a Linux builder first
  guests = lib.mapAttrs (
    _: test: lib.mapAttrs (_: node: node.system.build.toplevel) test.nodes
  ) suites;
}
