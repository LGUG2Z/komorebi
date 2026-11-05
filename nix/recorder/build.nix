# The crane derivations of komorebi-recorder on macOS and Linux, from the one Cargo workspace
# (nix/workspace-src.nix, platform "recorder": the recorder and the sources of the daemons it
# indexes). On Windows the recorder is built with the rest of the workspace (nix/windows/build.nix).
{ inputs, self }:
let
  lib' = import ../lib.nix { inherit inputs; };
  inherit (inputs) crane;
  # the recorder has a version of its own: it is released with komorebi, but is not komorebi
  version =
    (builtins.fromTOML (builtins.readFile ../../komorebi-recorder/Cargo.toml)).package.version;
in
{
  mkRecorderPackages =
    { pkgs }:
    let
      runtime = import ./runtime.nix { inherit pkgs; };
      toolchain = pkgs.rust-bin.fromRustupToolchainFile ../../rust-toolchain.toml;
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      inherit
        (import ../workspace-src.nix {
          inherit pkgs craneLib;
          repoRoot = ../..;
          platform = "recorder";
        })
        src
        ;

      lockFile = lib'.repoFile "Cargo.lock";
      tomlFile = lib'.repoFile "Cargo.toml";

      commonArgs = {
        pname = "komorebi-recorder";
        inherit version src;
        cargoExtraArgs = "--locked -p komorebi-recorder";
        # the lock and the root manifest as files of their own: a path inside the filtered source
        # would put all of it into the context of every derivation, and an edit of any file
        # would then change the dependencies
        cargoLock = lockFile;
        cargoToml = tomlFile;
        cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = lockFile; };
        strictDeps = true;
        # the commit of the komorebi source the build script indexes: a constant, so that the
        # dependencies and every check are the same derivations for every commit; only the build
        # below stamps the real one (outside a git checkout the build script is told it)
        KOMOREBI_COMMIT = "nix-check";
      }
      // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
        nativeBuildInputs = [
          pkgs.darwin.cctools
          # the tests that clone, and the ones that read the history of the source
          pkgs.git
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
      }
      # on Linux the video is encoded and decoded by GStreamer (the OS media stack there, as
      # AVFoundation is on a Mac), and found at run time through its plugin path
      // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
        nativeBuildInputs = [
          pkgs.pkg-config
          pkgs.makeWrapper
          # the tests that clone, and the ones that read the history of the source
          pkgs.git
          # the tests that record a video to read back make it with ffmpeg
          pkgs.ffmpeg-headless
        ];
        buildInputs = runtime.gstreamerPackages;
        postFixup = ''
          for bin in $out/bin/*; do
            if [ -f "$bin" ] && [ -x "$bin" ]; then
              wrapProgram "$bin" \
                --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : ${runtime.gstreamerPluginPath} \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtime.linuxRuntimeLibraries}
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
          KOMOREBI_COMMIT = self.rev or (pkgs.lib.removeSuffix "-dirty" (self.dirtyRev or "unknown"));
        }
      );

      # `komorebi-recorder-gui` is the same program as `komorebi-recorder` on a Mac; on Linux the
      # wrapped programs are all copied, so that the wrappers keep what they wrap
      komorebi-recorder = pkgs.runCommand "komorebi-recorder" { meta = fullBuild.meta // { }; } (
        if pkgs.stdenv.hostPlatform.isLinux then
          ''
            mkdir -p $out/bin
            cp -a ${fullBuild}/bin/. $out/bin/
          ''
        else
          ''
            mkdir -p $out/bin
            cp ${fullBuild}/bin/komorebi-recorder $out/bin/
          ''
      );
    in
    {
      inherit
        craneLib
        src
        individualCrateArgs
        fullBuild
        komorebi-recorder
        ;
    };
}
