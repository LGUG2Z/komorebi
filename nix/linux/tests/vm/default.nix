# The NixOS VM tests. Every directory under `suites/` is one test, `legacyPackages.<system>.checkMembers.vm-<tier>.vm-<name>`
# (Linux) and `legacyPackages.<system>.vm.<name>` (everywhere), so a new suite needs no edit to
# the flake. See docs/dev/vm-tests.md.
#
# `hostPkgs` is the pkgs of the machine that runs QEMU; the guests are always Linux.
# `komorebiModule` is the flake's NixOS module, `services.komorebi`, which every node imports: its
# overlay builds `komorebi` and `komorebic` for the guest's system. `scale` says how much slower
# than KVM that machine is (1.5 for KVM, nested or not, and for HVF on a Mac, and 8 for TCG), and every wait in the
# suites is multiplied by it.
{
  hostPkgs,
  komorebiModule,
  scale ? 1,
}:
let
  inherit (hostPkgs) lib;

  # `komorebi-recorder`, when the run is given one: its repository is private, so it is not an
  # input of the flake. `KOMOREBI_RECORDER=$(nix build --print-out-paths ...)` and `--impure`.
  recorder =
    let
      path = builtins.getEnv "KOMOREBI_RECORDER";
    in
    if path == "" then null else builtins.storePath path;

  # `E2E_ONLY=stacks,monocle` (with `--impure`; `KFDE_ONLY` is the older name) runs only the steps of
  # a suite whose name has one of the words; see `komorebi_vm.core.suite`
  only =
    let
      named = builtins.getEnv "E2E_ONLY";
    in
    if named != "" then named else builtins.getEnv "KFDE_ONLY";

  # the library the suites import, built for the python of the test driver
  komorebiVm =
    ps:
    ps.buildPythonPackage {
      pname = "komorebi-vm";
      version = "0.1.0";
      pyproject = true;
      src = lib.fileset.toSource {
        root = ./lib;
        # not the unit tests: the VM never runs them (setuptools does not package them), so
        # editing one must not rebuild every suite; `libCheck` keeps them
        fileset = lib.fileset.difference ./lib ./lib/tests;
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

      # the nodes import the module, `komorebiModule`, which adds its overlay to nixpkgs: the test
      # framework's pkgs, which cannot be changed, would refuse it
      node.specialArgs = { inherit komorebiModule; };
      node.pkgsReadOnly = false;

      nodes.machine = {
        # a suite's `node`, if it has one, adds to the node of its kind
        imports = [ (./nodes + "/${suite.kind}.nix") ] ++ lib.optional (suite ? node) suite.node;
        environment.systemPackages = lib.optional (recorder != null) recorder;
      };

      testScript = ''
        SCALE = ${toString scale}
        RECORDER = ${if recorder == null then "False" else "True"}
      ''
      + lib.optionalString (only != "") ''
        import os
        os.environ["E2E_ONLY"] = ${builtins.toJSON only}
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
