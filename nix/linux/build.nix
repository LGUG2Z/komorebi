# The crane derivations of the Linux (KDE Plasma) packages, from the Linux members of the one Cargo
# workspace (nix/workspace-src.nix).
{ inputs, self }:
let
  lib' = import ../lib.nix { inherit inputs; };
  inherit (lib') version;
  inherit (inputs) crane;
in
{
  mkKomorebiPackages =
    { pkgs }:
    let
      toolchain = pkgs.rust-bin.fromRustupToolchainFile ../../rust-toolchain.toml;
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      inherit
        (import ../workspace-src.nix {
          inherit pkgs craneLib;
          repoRoot = ../..;
          platform = "linux";
        })
        real
        src
        stubbedPackages
        ;

      lockFile = lib'.repoFile "Cargo.lock";
      tomlFile = lib'.repoFile "Cargo.toml";
      packages = "-p komorebi -p komorebi-linux -p komorebic -p komorebi-client -p komorebi-hotkeys";
      workspaceArgs = {
        pname = "komorebi-workspace-linux";
        inherit version;
        # the lock and the root manifest as files of their own: a path inside the filtered source
        # would put all of it into the context of every derivation, and an edit of any file
        # would then change the dependencies
        cargoLock = lockFile;
        cargoToml = tomlFile;
        cargoVendorDir = craneLib.vendorCargoDeps {
          cargoLock = lockFile;
          inherit (lib') cargoLockParsed;
        };
      };

      commonArgs = workspaceArgs // {
        cargoExtraArgs = "--locked ${packages}";
        inherit src;
        strictDeps = true;
        # The version crate wants a commit at compile time. It is a constant here, so that the
        # dependencies and every check are the same derivations for every commit; only the final
        # build below knows the real one.
        COMMIT_HASH = "nix-check";
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

      fullBuild = craneLib.buildPackage (
        individualCrateArgs
        // {
          COMMIT_HASH = self.rev or (pkgs.lib.removeSuffix "-dirty" (self.dirtyRev or "unknown"));
        }
      );

      # the same build with the constant commit of the checks: the VM tests install this one, so
      # that a new commit does not build the daemon again
      vmBuild = craneLib.buildPackage individualCrateArgs;

      extractBinaryOf =
        package: binaryName:
        pkgs.runCommand "komorebi-${binaryName}" { meta = package.meta // { }; } ''
          mkdir -p $out/bin
          cp ${package}/bin/${binaryName} $out/bin/
        '';
      extractBinary = extractBinaryOf fullBuild;
    in
    {
      inherit
        craneLib
        src
        real
        workspaceArgs
        stubbedPackages
        packages
        commonArgs
        individualCrateArgs
        fullBuild
        vmBuild
        ;
      komorebi = extractBinary "komorebi";
      komorebic = extractBinary "komorebic";
      komorebiForVm = extractBinaryOf vmBuild "komorebi";
      komorebicForVm = extractBinaryOf vmBuild "komorebic";
      # komorebic with the constant commit of the checks, whose output the schemas are
      # checked against: it is the same for every commit
      komorebicForChecks = craneLib.buildPackage (
        individualCrateArgs
        // {
          pname = "komorebic-for-checks";
          cargoExtraArgs = "--package komorebic --bin komorebic";
        }
      );
    };
}
