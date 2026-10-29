{
  description = "komorebi-recorder: synchronized screen, log and trace capture for komorebi bug reproduction";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    git-hooks-nix.url = "github:cachix/git-hooks.nix";
    git-hooks-nix.inputs.nixpkgs.follows = "nixpkgs";

    # komorebi for Mac and komorebi for Windows: only their sources, which the recorder indexes so
    # that `code` can map a line of a session's log to the statement that wrote it
    komorebi-mac = {
      url = "github:LGUG2Z/komorebi-for-mac";
      flake = false;
    };
    komorebi-windows = {
      url = "github:LGUG2Z/komorebi";
      flake = false;
    };
  };

  outputs =
    inputs@{
      nixpkgs,
      flake-parts,
      crane,
      rust-overlay,
      ...
    }:
    let
      windows = import ./nix/windows.nix;

      # the sources the recorder indexes for each flavor of komorebi (see komorebi-recorder/build.rs),
      # which are also the trees the tests that read komorebi's source read
      sourceEnv = {
        KOMOREBI_MAC_SRC = "${inputs.komorebi-mac}";
        KOMOREBI_MAC_COMMIT = inputs.komorebi-mac.rev;
        KOMOREBI_WINDOWS_SRC = "${inputs.komorebi-windows}";
        KOMOREBI_WINDOWS_COMMIT = inputs.komorebi-windows.rev;
      };

      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

      # what the build needs of the repository: Cargo sources, and the files the crate embeds or
      # its tests read
      mkSource =
        { pkgs, craneLib }:
        pkgs.lib.cleanSourceWith {
          src = ./.;
          filter =
            path: type:
            (craneLib.filterCargoSources path type)
            || (pkgs.lib.hasSuffix "/komorebi-recorder/src/guide.md" path)
            || (pkgs.lib.hasInfix "/komorebi-recorder/tests/fixtures/" path)
            # the installer, which `tests/packaging.rs` checks
            || (pkgs.lib.hasInfix "/wix/" path)
            # the icon, which `src/icon.rs` embeds and `build.rs` compiles into the Windows executables
            || (pkgs.lib.hasInfix "/komorebi-recorder/assets/" path);
        };

      mkPackages =
        { pkgs }:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          src = mkSource { inherit pkgs craneLib; };

          commonArgs = {
            inherit src version;
            pname = "komorebi-recorder";
            strictDeps = true;
            inherit (sourceEnv)
              KOMOREBI_MAC_SRC
              KOMOREBI_MAC_COMMIT
              KOMOREBI_WINDOWS_SRC
              KOMOREBI_WINDOWS_COMMIT
              ;
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
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

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            doDoc = false;
          };

          fullBuild = craneLib.buildPackage individualCrateArgs;

          # `komorebi-recorder-gui` is the same program as `komorebi-recorder` on a Mac
          komorebi-recorder = pkgs.runCommand "komorebi-recorder" { meta = fullBuild.meta // { }; } ''
            mkdir -p $out/bin
            cp ${fullBuild}/bin/komorebi-recorder $out/bin/
          '';
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

      # komorebi-recorder for Windows, cross-compiled from this host
      mkWindowsPackages =
        { pkgs, windowsSdk }:
        let
          toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          msvcEnv = windows.mkMsvcEnv { inherit pkgs windowsSdk; };
          src = mkSource { inherit pkgs craneLib; };

          commonArgs = {
            inherit src version;
            pname = "komorebi-recorder-windows";
            strictDeps = true;
            cargoExtraArgs = "--locked -p komorebi-recorder";
            inherit (sourceEnv)
              KOMOREBI_MAC_SRC
              KOMOREBI_MAC_COMMIT
              KOMOREBI_WINDOWS_SRC
              KOMOREBI_WINDOWS_COMMIT
              ;

            nativeBuildInputs = [
              pkgs.llvmPackages.clang-unwrapped
              pkgs.llvmPackages.lld
              pkgs.llvmPackages.llvm
            ];
          }
          // msvcEnv;

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            doDoc = false;
          };

          komorebi-recorder = craneLib.buildPackage individualCrateArgs;
        in
        {
          inherit
            craneLib
            individualCrateArgs
            msvcEnv
            komorebi-recorder
            ;
        };

      mkPkgs =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem =
        { config, system, ... }:
        let
          pkgs = mkPkgs system;
          build = mkPackages { inherit pkgs; };
          windowsSdk = windows.mkWindowsSdk {
            inherit pkgs;
            inherit (windows) windowsSdkVersion windowsCrtVersion;
          };
          windowsBuild = mkWindowsPackages { inherit pkgs windowsSdk; };
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
          rustToolchainWithNightlyRustfmt = pkgs.symlinkJoin {
            name = "rust-toolchain-with-nightly-rustfmt";
            paths = [
              nightlyRustfmt
              rustToolchain
            ];
          };
        in
        {
          treefmt = {
            projectRootFile = "flake.nix";
            programs = {
              deadnix.enable = true;
              just.enable = true;
              nixfmt.enable = true;
              taplo.enable = true;
              rustfmt = {
                enable = true;
                package = pkgs.rust-bin.nightly.latest.rustfmt;
              };
            };
          };

          checks = {
            komorebi-recorder-clippy = build.craneLib.cargoClippy (
              build.individualCrateArgs
              // {
                cargoClippyExtraArgs = "--all-targets -- -D warnings";
              }
            );

            komorebi-recorder-fmt = build.craneLib.cargoFmt {
              inherit (build) src;
              pname = "komorebi-recorder";
            };

            komorebi-recorder-toml-fmt = build.craneLib.taploFmt {
              src = pkgs.lib.sources.sourceFilesBySuffices build.src [ ".toml" ];
              pname = "komorebi-recorder";
            };

            komorebi-recorder-deny = build.craneLib.cargoDeny {
              inherit (build) src;
              pname = "komorebi-recorder";
            };

            komorebi-recorder-nextest = build.craneLib.cargoNextest build.individualCrateArgs;

            # `--all-targets` so that every line of `cfg(windows)` code, tests included, is type-checked
            # without a Windows machine
            komorebi-recorder-windows-clippy = windowsBuild.craneLib.cargoClippy (
              windowsBuild.individualCrateArgs
              // {
                cargoClippyExtraArgs = "--all-targets -- -D warnings";
              }
            );
          };

          packages = {
            inherit (build) komorebi-recorder;
            inherit windowsSdk;
            komorebi-recorder-windows = windowsBuild.komorebi-recorder;
            default = build.komorebi-recorder;
          };

          apps = {
            komorebi-recorder = {
              type = "app";
              program = "${build.komorebi-recorder}/bin/komorebi-recorder";
            };
            default = {
              type = "app";
              program = "${build.komorebi-recorder}/bin/komorebi-recorder";
            };
          };

          devShells.default = pkgs.mkShell {
            name = "komorebi-recorder";

            RUST_BACKTRACE = "full";

            inherit (sourceEnv)
              KOMOREBI_MAC_SRC
              KOMOREBI_MAC_COMMIT
              KOMOREBI_WINDOWS_SRC
              KOMOREBI_WINDOWS_COMMIT
              ;

            inputsFrom = [ build.fullBuild ];

            packages = [
              rustToolchainWithNightlyRustfmt

              pkgs.cargo-deny
              pkgs.cargo-nextest
              # `komorebi-recorder frames` and `selftest` extract frames with ffmpeg; it is never
              # used for capture and is not bundled into the packaged binary
              pkgs.ffmpeg-headless
              pkgs.jq
              pkgs.just
              pkgs.ripgrep
              # `just icon` renders the watermelon
              pkgs.resvg
            ];
          };

          # For working on the Windows side from a Mac: `nix develop .#windows`, then plain
          # `cargo clippy --all-targets` (the target is already `x86_64-pc-windows-msvc`).
          devShells.windows = pkgs.mkShell {
            name = "komorebi-recorder-windows";

            RUST_BACKTRACE = "full";

            inherit (windowsBuild.msvcEnv)
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER
              CC_x86_64_pc_windows_msvc
              CXX_x86_64_pc_windows_msvc
              AR_x86_64_pc_windows_msvc
              CFLAGS_x86_64_pc_windows_msvc
              CXXFLAGS_x86_64_pc_windows_msvc
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
              CARGO_BUILD_TARGET
              ;
            inherit (sourceEnv)
              KOMOREBI_MAC_SRC
              KOMOREBI_MAC_COMMIT
              KOMOREBI_WINDOWS_SRC
              KOMOREBI_WINDOWS_COMMIT
              ;

            packages = [
              (rustToolchain.override { targets = [ "x86_64-pc-windows-msvc" ]; })
              pkgs.llvmPackages.clang-unwrapped # provides clang-cl
              pkgs.llvmPackages.lld # provides lld-link
              pkgs.llvmPackages.llvm # provides llvm-lib
              pkgs.cargo-deny
              pkgs.just
            ];
          };

          pre-commit = {
            check.enable = true;
            settings.hooks.treefmt = {
              enable = true;
              package = config.treefmt.build.wrapper;
              pass_filenames = false;
            };
          };
        };

      flake = {
        overlays.default =
          final: _:
          let
            pkgs = mkPkgs final.system;
            build = mkPackages { inherit pkgs; };
          in
          {
            inherit (build) komorebi-recorder;
          };
      };
    };
}
