# The checks of the flake as separate sets.
#
# The platform modules (nix/{windows,linux,macos}/flake-module.nix) define their checks as
# `allChecks.<name>`; this module says which SET each one is in and builds from that
#
#   legacyPackages.<system>.checkSets.<set>     one derivation per set: a linkFarm that builds its members
#   legacyPackages.<system>.checkMembers.<set>  the members of a set, one derivation each
#   checks.<system>                             the members of the sets that heavyCheckSets does not name
#
# so `nix build .#checkSets.linux-lint` builds one set and evaluates nothing else, and `nix flake check`
# evaluates only what `checks` has. See docs/dev/flake-checks.md.
{ lib, ... }:
let
  # The checks of each set, by name: a name that a system does not have (the macOS checks on Linux)
  # is not a member there. A check that is in no set, or in two, is an error (below).
  sets = {
    fmt = [
      "treefmt"
      "pre-commit"
      "windows-fmt"
      "windows-toml-fmt"
      "linux-fmt"
      "linux-toml-fmt"
      "macos-fmt"
      "macos-toml-fmt"
    ];
    deny = [
      "windows-deny"
      "linux-deny"
      "macos-deny"
    ];
    windows-lint = [ "windows-clippy" ];
    windows-test = [
      "windows-nextest"
      "windows-e2e-tests"
    ];
    linux-lint = [
      "linux-clippy"
      "linux-doc"
    ];
    linux-test = [
      "linux-nextest"
      "linux-bridge-js"
      "linux-keybindings-fixtures"
      "linux-wsl2-probe"
      "linux-wsl2-probe-dry-run"
      "linux-kde-rounded-corners-komorebi-install"
      "linux-kwin-effect-geometry-change-komorebi-install"
      "vm-lib"
    ];
    linux-options = [
      "linux-nix-options-validation"
      "linux-schema-fresh"
    ];
    macos-lint = [ "macos-clippy" ];
    macos-test = [ "macos-nextest" ];
    macos-options = [
      "macos-nix-options-validation"
      "macos-darwin-module-validation"
    ];
    # the demo guest of `run-kde` is instantiated to check the command: about 1.5 GB to evaluate
    linux-demo = [
      "linux-run-kde"
      "linux-run-kde-doctor"
      "linux-run-kde-dry-run"
    ];
    # about 10 GB each to evaluate: tens of NixOS systems
    linux-nixos-eval = [
      "linux-nixos-module-eval"
      "linux-plasma-guests-eval"
    ];
  };

  # the sets sized by what is on the system: the VM suites of each tier (nix/linux/tests/vm)
  suiteSets =
    tiers:
    lib.mapAttrs' (tier: names: lib.nameValuePair "vm-${tier}" (map (name: "vm-${name}") names)) tiers;

  # the sets that `nix flake check` does not run: evaluating their members takes more memory than a
  # check of the repository should (docs/dev/flake-checks.md has the numbers)
  heavyCheckSets = [
    "linux-demo"
    "linux-nixos-eval"
    "vm-fast"
    "vm-slow"
  ];
in
{
  perSystem =
    {
      config,
      pkgs,
      ...
    }:
    let
      allChecks = config.allChecks;
      # the VM suites exist where `vmTiers` does (the Linux systems and Apple silicon)
      tiers = config.legacyPackages.vmTiers or { };
      namesOf = lib.attrsets.recursiveUpdate sets (suiteSets tiers);
      # the members of a set on this system
      members = lib.mapAttrs (
        _: names: lib.getAttrs (builtins.filter (name: allChecks ? ${name}) names) allChecks
      ) namesOf;
      nonEmpty = lib.filterAttrs (_: ms: ms != { }) members;

      placed = lib.concatLists (lib.attrValues (lib.mapAttrs (_: lib.attrNames) members));
      unplaced = builtins.filter (name: !(builtins.elem name placed)) (lib.attrNames allChecks);
      twice = lib.filter (name: builtins.length (lib.filter (n: n == name) placed) > 1) placed;
      mustBeInOneSet = lib.assertMsg (unplaced == [ ] && twice == [ ]) (
        "every check is in exactly one set of nix/checks.nix; in none: ${toString unplaced}; in several: ${toString twice}"
      );
    in
    {
      options.allChecks = lib.mkOption {
        type = lib.types.lazyAttrsOf lib.types.raw;
        default = { };
        description = "Every check of the system by name, whichever set it is in.";
      };

      config = {
        checks = assert mustBeInOneSet;
          lib.concatMapAttrs (set: ms: if builtins.elem set heavyCheckSets then { } else ms) nonEmpty;

        legacyPackages = {
          checkMembers = nonEmpty;
          checkSets = lib.mapAttrs (
            set: ms:
            pkgs.linkFarm "checks-${set}" (lib.mapAttrsToList (name: path: { inherit name path; }) ms)
          ) nonEmpty;
        };
      };
    };
}
