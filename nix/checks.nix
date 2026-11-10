# The checks of the flake as separate sets.
#
# The platform modules (nix/{windows,linux,macos}/flake-module.nix) define their checks as
# `allChecks.<name>`; this module says which SET each one is in and builds from that
#
#   legacyPackages.<system>.checkSets.<set>     one derivation per set: a linkFarm that builds its members
#   legacyPackages.<system>.checkMembers.<set>  the members of a set, one derivation each
#   checks.<system>                             the members of the sets of defaultCheckSets (below)
#   checkSets.cheap, checkSets.all              the default sets together, and every set together
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
    # the Python of the Windows e2e that needs no guest, and the Windows crates' test executables,
    # cross-built: they cannot run on a Linux builder, they run in the guest (`nix run .#windows-nextest`)
    windows-test = [
      "windows-e2e-tests"
      "windows-nextest-build"
    ];
    # the test executables of the WHOLE Windows workspace, not only the windows and os_sensitive packages that
    # `windows-test` builds (tools/test-partition.json): for the owner to run by hand in the guest
    # (`nix run .#windows-nextest-full`). Not in defaultCheckSets, so no pull request builds it.
    windows-test-full = [ "windows-nextest-full-build" ];
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
    nixgen-lint = [ "nixgen-clippy" ];
    nixgen-test = [ "nixgen-nextest" ];
    docgen-lint = [ "docgen-clippy" ];
    docgen-test = [ "docgen-nextest" ];
    recorder-lint = [ "recorder-clippy" ];
    recorder-test = [ "recorder-nextest" ];
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

  # THE LIST that composes `checks`: what `nix flake check` and a pull request run. The heavy sets
  # (linux-demo, linux-nixos-eval, vm-fast, vm-slow: 1.5 to 19 GB to evaluate each, see
  # docs/dev/flake-checks.md) are left out and stay reachable as `.#checkSets.<set>`. Choosing what
  # `nix flake check` runs is an edit of this list.
  defaultCheckSets = [
    "fmt"
    "deny"
    "windows-lint"
    "windows-test"
    "linux-lint"
    "linux-test"
    "linux-options"
    "macos-lint"
    "macos-test"
    "macos-options"
    "recorder-lint"
    "recorder-test"
    "nixgen-lint"
    "nixgen-test"
    "docgen-lint"
    "docgen-test"
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

      config =
        let
          defaultMembers = lib.concatMapAttrs (_: ms: ms) (
            lib.getAttrs (builtins.filter (set: nonEmpty ? ${set}) defaultCheckSets) nonEmpty
          );
        in
        {
          checks =
            assert mustBeInOneSet;
            defaultMembers;

          legacyPackages = {
            checkMembers = nonEmpty;
            # `cheap` is what `checks` has, `all` every check there is
            checkSets =
              lib.mapAttrs
                (
                  set: ms: pkgs.linkFarm "checks-${set}" (lib.mapAttrsToList (name: path: { inherit name path; }) ms)
                )
                (
                  nonEmpty
                  // {
                    cheap = defaultMembers;
                    all = allChecks;
                  }
                );
          };
        };
    };
}
