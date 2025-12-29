{
  description = "komorebi for Windows";

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
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-parts,
      crane,
      rust-overlay,
      ...
    }:
    let
      windowsSdkVersion = "10.0.26100";
      windowsCrtVersion = "14.44.17.14";

      mkWindowsSdk =
        pkgs:
        pkgs.stdenvNoCC.mkDerivation {
          name = "windows-sdk-${windowsSdkVersion}-crt-${windowsCrtVersion}";

          nativeBuildInputs = [ pkgs.xwin ];

          outputHashAlgo = "sha256";
          outputHashMode = "recursive";
          outputHash = "sha256-6cLS5q1BDRpLPScfmmKpTTEHUzsgKTKD1+mKvGX9Deo=";

          buildCommand = ''
            export HOME=$(mktemp -d)
            xwin --accept-license \
              --sdk-version ${windowsSdkVersion} \
              --crt-version ${windowsCrtVersion} \
              splat --output $out
          '';
        };

      mkMsvcEnv =
        { pkgs, windowsSdk }:
        let
          clangVersion = pkgs.lib.versions.major pkgs.llvmPackages.clang.version;
        in
        {
          # linker for the windows target
          CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "lld-link";

          # c/c++ compiler
          CC_x86_64_pc_windows_msvc = "clang-cl";
          CXX_x86_64_pc_windows_msvc = "clang-cl";
          AR_x86_64_pc_windows_msvc = "llvm-lib";

          # IMPORTANT: libclang include path MUST come first to avoid header conflicts
          CFLAGS_x86_64_pc_windows_msvc = builtins.concatStringsSep " " [
            "--target=x86_64-pc-windows-msvc"
            "-Wno-unused-command-line-argument"
            "-fuse-ld=lld-link"
            "/imsvc${pkgs.llvmPackages.libclang.lib}/lib/clang/${clangVersion}/include"
            "/imsvc${windowsSdk}/crt/include"
            "/imsvc${windowsSdk}/sdk/include/ucrt"
            "/imsvc${windowsSdk}/sdk/include/um"
            "/imsvc${windowsSdk}/sdk/include/shared"
          ];

          CXXFLAGS_x86_64_pc_windows_msvc = builtins.concatStringsSep " " [
            "--target=x86_64-pc-windows-msvc"
            "-Wno-unused-command-line-argument"
            "-fuse-ld=lld-link"
            "/imsvc${pkgs.llvmPackages.libclang.lib}/lib/clang/${clangVersion}/include"
            "/imsvc${windowsSdk}/crt/include"
            "/imsvc${windowsSdk}/sdk/include/ucrt"
            "/imsvc${windowsSdk}/sdk/include/um"
            "/imsvc${windowsSdk}/sdk/include/shared"
          ];

          # target-specific rust flags with linker flavor and library search paths
          CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = builtins.concatStringsSep " " [
            "-Clinker-flavor=lld-link"
            "-Lnative=${windowsSdk}/crt/lib/x86_64"
            "-Lnative=${windowsSdk}/sdk/lib/um/x86_64"
            "-Lnative=${windowsSdk}/sdk/lib/ucrt/x86_64"
          ];

          # cargo target
          CARGO_BUILD_TARGET = "x86_64-pc-windows-msvc";
        };

      mkKomorebiPackages =
        { pkgs, windowsSdk }:
        let
          # toolchain with windows msvc target
          toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          version = "0.1.0";

          msvcEnv = mkMsvcEnv { inherit pkgs windowsSdk; };

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasInfix "/docs/" path)
              || (builtins.match ".*/docs/.*" path != null);
          };

          commonArgs = {
            inherit src version;
            strictDeps = true;
            COMMIT_HASH = self.rev or (pkgs.lib.removeSuffix "-dirty" self.dirtyRev);

            # build inputs for cross-compilation
            nativeBuildInputs = [
              pkgs.llvmPackages.clang-unwrapped
              pkgs.llvmPackages.lld
              pkgs.llvmPackages.llvm
            ];

            # cross-compilation environment
            inherit (msvcEnv)
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER
              CC_x86_64_pc_windows_msvc
              CXX_x86_64_pc_windows_msvc
              AR_x86_64_pc_windows_msvc
              CFLAGS_x86_64_pc_windows_msvc
              CXXFLAGS_x86_64_pc_windows_msvc
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
              CARGO_BUILD_TARGET
              ;
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            doDoc = false;
          };

          fullBuild = craneLib.buildPackage (
            individualCrateArgs
            // {
              pname = "komorebi-workspace";
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
                cp ${fullBuild}/bin/${binaryName}.exe $out/bin/
              '';
        in
        {
          inherit
            craneLib
            src
            individualCrateArgs
            fullBuild
            msvcEnv
            ;
          komorebi = extractBinary "komorebi";
          komorebic = extractBinary "komorebic";
          komorebic-no-console = extractBinary "komorebic-no-console";
          komorebi-bar = extractBinary "komorebi-bar";
          komorebi-gui = extractBinary "komorebi-gui";
          komorebi-shortcuts = extractBinary "komorebi-shortcuts";
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
        "x86_64-linux"
        "aarch64-linux"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem =
        { config, system, ... }:
        let
          pkgs = mkPkgs system;
          windowsSdk = mkWindowsSdk pkgs;
          build = mkKomorebiPackages { inherit pkgs windowsSdk; };

          # toolchain with windows target and nightly rustfmt
          rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
          rustToolchainWithNightlyRustfmt = pkgs.symlinkJoin {
            name = "rust-toolchain-with-nightly-rustfmt";
            paths = [
              nightlyRustfmt
              rustToolchain
            ];
          };
          nightlyToolchain = pkgs.rust-bin.nightly.latest.default.override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          cargo-udeps = pkgs.writeShellScriptBin "cargo-udeps" ''
            export PATH="${nightlyToolchain}/bin:$PATH"
            exec ${pkgs.cargo-udeps}/bin/cargo-udeps "$@"
          '';
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
            komorebi-workspace-clippy = build.craneLib.cargoClippy (
              build.individualCrateArgs
              // {
                cargoClippyExtraArgs = "--all-targets -- -D warnings";
              }
            );

            komorebi-workspace-fmt = build.craneLib.cargoFmt {
              inherit (build) src;
            };

            komorebi-workspace-toml-fmt = build.craneLib.taploFmt {
              src = pkgs.lib.sources.sourceFilesBySuffices build.src [ ".toml" ];
            };

            komorebi-workspace-deny = build.craneLib.cargoDeny {
              inherit (build) src;
            };

            komorebi-workspace-nextest = build.craneLib.cargoNextest build.individualCrateArgs;
          };

          packages = {
            inherit (build)
              komorebi
              komorebic
              komorebic-no-console
              komorebi-bar
              komorebi-gui
              komorebi-shortcuts
              ;
            inherit windowsSdk;
            komorebi-full = build.fullBuild;
            default = build.fullBuild;
          };

          apps = {
            komorebi = {
              type = "app";
              program = "${build.komorebi}/bin/komorebi.exe";
            };
            komorebic = {
              type = "app";
              program = "${build.komorebic}/bin/komorebic.exe";
            };
            komorebic-no-console = {
              type = "app";
              program = "${build.komorebic-no-console}/bin/komorebic-no-console.exe";
            };
            komorebi-bar = {
              type = "app";
              program = "${build.komorebi-bar}/bin/komorebi-bar.exe";
            };
            komorebi-gui = {
              type = "app";
              program = "${build.komorebi-gui}/bin/komorebi-gui.exe";
            };
            komorebi-shortcuts = {
              type = "app";
              program = "${build.komorebi-shortcuts}/bin/komorebi-shortcuts.exe";
            };
            default = {
              type = "app";
              program = "${build.fullBuild}/bin/komorebi.exe";
            };
          };

          devShells.default = pkgs.mkShell {
            name = "komorebi";

            RUST_BACKTRACE = "full";

            # cross-compilation environment
            inherit (build.msvcEnv)
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER
              CC_x86_64_pc_windows_msvc
              CXX_x86_64_pc_windows_msvc
              AR_x86_64_pc_windows_msvc
              CFLAGS_x86_64_pc_windows_msvc
              CXXFLAGS_x86_64_pc_windows_msvc
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
              CARGO_BUILD_TARGET
              ;

            packages = [
              rustToolchainWithNightlyRustfmt
              cargo-udeps

              # cross-compilation tooling
              pkgs.llvmPackages.clang-unwrapped # provides clang-cl
              pkgs.llvmPackages.lld # provides lld-link
              pkgs.llvmPackages.llvm # provides llvm-lib

              pkgs.cargo-deny
              pkgs.cargo-nextest
              pkgs.cargo-outdated
              pkgs.jq
              pkgs.just
              pkgs.prettier
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
    };
}
