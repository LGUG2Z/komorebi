# The crane derivations of the macOS packages, from the macOS members of the one Cargo workspace
# (nix/workspace-src.nix).
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
          platform = "macos";
        })
        real
        src
        stubbedPackages
        ;

      lockFile = lib'.repoFile "Cargo.lock";
      tomlFile = lib'.repoFile "Cargo.toml";
      packages = "-p komorebi -p komorebi-macos -p komorebi-macos-core -p komorebic -p komorebi-client -p komorebi-bar";
      workspaceArgs = {
        pname = "komorebi-workspace-macos";
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
        # a constant, so that the dependencies and every check are the same derivations for
        # every commit; only fullBuild stamps the real one
        COMMIT_HASH = "nix-check";
        nativeBuildInputs = [
          pkgs.darwin.cctools
        ];
        postFixup = ''
          for bin in $out/bin/*; do
            if [ -f "$bin" ] && [ -x "$bin" ]; then
              # Get all linked libraries and fix libiconv references
              otool -L "$bin" | grep -o '/nix/store/[^/]*/lib/libiconv[^ ]*' | while read -r lib; do
                install_name_tool -change "$lib" /usr/lib/libiconv.dylib "$bin"
              done
            fi
          done
        '';
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

      # the programs the e2e guest runs besides the daemon: the window to tile and the agent that answers
      # the host. They are not part of fullBuild, which is what users get.
      e2eBuild = craneLib.buildPackage (
        individualCrateArgs
        // {
          pname = "komorebi-e2e-macos";
          cargoExtraArgs = "--locked -p komorebi-testwin -p komorebi-e2e-agent";
          COMMIT_HASH = "e2e";
        }
      );

      extractBinary =
        binaryName:
        pkgs.runCommand "komorebi-${binaryName}"
          {
            meta = fullBuild.meta // { };
          }
          ''
            mkdir -p $out/bin
            cp ${fullBuild}/bin/${binaryName} $out/bin/
          '';
    in
    {
      inherit
        craneLib
        src
        real
        workspaceArgs
        stubbedPackages
        packages
        individualCrateArgs
        fullBuild
        e2eBuild
        ;
      komorebi = extractBinary "komorebi";
      komorebic = extractBinary "komorebic";
      komorebi-bar = extractBinary "komorebi-bar";
    };
}
