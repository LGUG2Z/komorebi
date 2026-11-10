# The crane derivations of schemars-docgen, the tool that writes the reference pages of the docs site
# (starlight/) from the JSON Schemas, from the one Cargo workspace (nix/workspace-src.nix, platform "docgen": the tool and nothing
# else). It is pure Rust, so one build serves every system.
{ inputs }:
let
  lib' = import ../lib.nix { inherit inputs; };
  inherit (inputs) crane;
  version =
    (builtins.fromTOML (builtins.readFile ../../crates/schemars-docgen/Cargo.toml)).package.version;
in
{
  mkDocgenPackages =
    { pkgs }:
    let
      toolchain = pkgs.rust-bin.fromRustupToolchainFile ../../rust-toolchain.toml;
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      inherit
        (import ../workspace-src.nix {
          inherit pkgs craneLib;
          repoRoot = ../..;
          platform = "docgen";
        })
        src
        ;

      lockFile = lib'.repoFile "Cargo.lock";
      tomlFile = lib'.repoFile "Cargo.toml";

      commonArgs = {
        pname = "schemars-docgen";
        inherit version src;
        cargoExtraArgs = "--locked -p schemars-docgen";
        # the lock and the root manifest as files of their own: a path inside the filtered source
        # would put all of it into the context of every derivation, and an edit of any file
        # would then change the dependencies
        cargoLock = lockFile;
        cargoToml = tomlFile;
        cargoVendorDir = craneLib.vendorCargoDeps {
          cargoLock = lockFile;
          inherit (lib') cargoLockParsed;
        };
        strictDeps = true;
      };

      # the dummy source is of the manifests alone (nix/lib.nix): the dependencies are the same
      # derivations for every source file and every stub, and nothing is built at evaluation
      cargoArtifacts = craneLib.buildDepsOnly (
        builtins.removeAttrs commonArgs [ "src" ]
        // {
          dummySrc = craneLib.mkDummySrc {
            src = lib'.manifestSource pkgs.lib;
            cargoLock = lockFile;
          };
        }
      );

      individualCrateArgs = commonArgs // {
        inherit cargoArtifacts;
        doCheck = false;
        doDoc = false;
      };

      schemars-docgen = craneLib.buildPackage individualCrateArgs;
    in
    {
      inherit
        craneLib
        src
        individualCrateArgs
        schemars-docgen
        ;
    };
}
