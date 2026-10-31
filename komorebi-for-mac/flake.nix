{
  description = "komorebi for Mac";

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

  # The outputs are a function of the repository they are built from, so that the root flake can
  # build them with the whole repository as the source (the Rust workspace here depends on the
  # crates of the root workspace by path) and with its own `self`. `lib.mkOutputs` is that function.
  # The outputs of this flake on its own use the directory it is in: that evaluates everything and
  # builds what is not Rust, but cannot build the Rust workspace.
  outputs =
    inputs@{ self, ... }:
    let
      mkOutputs =
        {
          inputs,
          self,
          # the source of the repository, and the path of this tree in it
          repoRoot,
          platformDir,
        }:
        let
          inherit (inputs)
            nixpkgs
            flake-parts
            crane
            rust-overlay
            ;
          rootCrates = [
            "crates"
            "komorebi-layouts"
            "komorebi-scrolling"
            "komorebi-themes"
          ];
          mkKomorebiPackages =
            { pkgs }:
            let
              toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
              craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
              version = "0.1.0";

              # the repository, cut down to this workspace and the crates of the root workspace it depends
              # on by path, with the root manifest those crates inherit their dependencies from
              real = pkgs.lib.cleanSourceWith {
                src = repoRoot;
                filter =
                  path: type:
                  let
                    relative = pkgs.lib.removePrefix "${toString repoRoot}/" path;
                    inTree =
                      platformDir == "" || relative == platformDir || pkgs.lib.hasPrefix "${platformDir}/" relative;
                    rootCrate = builtins.any (
                      crate: relative == crate || pkgs.lib.hasPrefix "${crate}/" relative
                    ) rootCrates;
                    # the unified komorebic package: its manifest, build script, main and the macOS module
                    komorebic =
                      builtins.elem relative [
                        "komorebic"
                        "komorebic/Cargo.toml"
                        "komorebic/build.rs"
                        "komorebic/src"
                        "komorebic/src/main.rs"
                      ]
                      || pkgs.lib.hasPrefix "komorebic/src/macos" relative;
                  in
                  ((rootCrate || komorebic) && !(pkgs.lib.hasSuffix "/Cargo.lock" path) || relative == "Cargo.toml" || relative == "Cargo.lock" || inTree)
                  && (
                    (craneLib.filterCargoSources path type)
                    || (pkgs.lib.hasInfix "/docs/" path)
                    || (builtins.match ".*/docs/.*" path != null)
                    # embedded by include_str! in komorebic
                    || (pkgs.lib.hasSuffix "/komorebic/src/macos/applications.json" path)
                  );
              };

              # the platform is a part of the one workspace of the repository: cargo loads every member (the
              # others are stubs, see nix/workspace-src.nix) and is told which packages are this platform's
              src = import (repoRoot + "/nix/workspace-src.nix") {
                inherit pkgs repoRoot real;
                isReal =
                  member:
                  pkgs.lib.hasPrefix "${platformDir}/" member
                  || member == "komorebic"
                  || builtins.any (crate: member == crate || pkgs.lib.hasPrefix "${crate}/" member) rootCrates;
              };
              packages = "-p komorebi-macos -p komorebic -p komorebi-client-macos -p komorebi-bar-macos";
              workspaceArgs = {
                pname = "komorebi-workspace";
                inherit version;
                cargoLock = "${real}/Cargo.lock";
                cargoToml = "${real}/Cargo.toml";
                # vendored from the filtered source, not the stubbed one: no build is needed to evaluate
                cargoVendorDir = craneLib.vendorCargoDeps { src = real; };
              };

              commonArgs = workspaceArgs // {
                cargoExtraArgs = "--locked ${packages}";
                inherit src version;
                strictDeps = true;
                COMMIT_HASH = self.rev or (pkgs.lib.removeSuffix "-dirty" (self.dirtyRev or "unknown"));
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
                    cp ${fullBuild}/bin/${binaryName} $out/bin/
                  '';
            in
            {
              inherit
                craneLib
                src
                real
                workspaceArgs
                packages
                individualCrateArgs
                fullBuild
                ;
              komorebi = extractBinary "komorebi";
              komorebic = extractBinary "komorebic";
              komorebi-bar = extractBinary "komorebi-bar";
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
          ];

          imports = [
            inputs.treefmt-nix.flakeModule
            inputs.git-hooks-nix.flakeModule
          ];

          perSystem =
            { config, system, ... }:
            let
              pkgs = mkPkgs system;
              build = mkKomorebiPackages { inherit pkgs; };
              rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
              nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
              rustToolchainWithNightlyRustfmt = pkgs.symlinkJoin {
                name = "rust-toolchain-with-nightly-rustfmt";
                paths = [
                  nightlyRustfmt
                  rustToolchain
                ];
              };
              nightlyToolchain = pkgs.rust-bin.nightly.latest.default;
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
                komorebi-workspace-clippy = build.craneLib.cargoClippy build.individualCrateArgs;

                komorebi-workspace-fmt = build.craneLib.cargoFmt (build.workspaceArgs // { inherit (build) src; cargoExtraArgs = build.packages; });

                komorebi-workspace-toml-fmt = build.craneLib.taploFmt (
                  build.workspaceArgs
                  // {
                    src = pkgs.lib.sources.sourceFilesBySuffices build.real [ ".toml" ];
                  }
                );

                # cargo deny sees every member of the workspace: leave out the other platforms' packages
                # and use this tree's policy
                komorebi-workspace-deny = build.craneLib.cargoDeny (
                  build.workspaceArgs
                  // {
                    inherit (build) src;
                    cargoDenyExtraArgs =
                      "--workspace "
                      + pkgs.lib.concatMapStringsSep " " (p: "--exclude ${p}") [
                        "komorebi-windows"
                        "komorebi-client"
                        "komorebi-gui"
                        "komorebic-no-console"
                        "komorebi-bar"
                        "komorebi-shortcuts"
                        "komorebi-e2e-agent"
                        "komorebi-testwin"
                        "komorebi-linux"
                        "komorebic-linux"
                        "komorebi-client-linux"
                        "komorebi-hotkeys"
                        "version-linux"
                      ];
                    cargoDenyChecks = "--config ${platformDir}/deny.toml bans licenses sources";
                  }
                );

                komorebi-workspace-nextest = build.craneLib.cargoNextest build.individualCrateArgs;

                nix-options-validation = import ./nix/tests { inherit pkgs; };

                darwin-module-validation = import ./nix/tests/darwin-module.nix { inherit pkgs; };
              };

              packages = {
                inherit (build) komorebi komorebic komorebi-bar;
                komorebi-full = build.fullBuild;
                default = build.fullBuild;
              };

              apps = {
                komorebi = {
                  type = "app";
                  program = "${build.komorebi}/bin/komorebi";
                };
                komorebic = {
                  type = "app";
                  program = "${build.komorebic}/bin/komorebic";
                };
                komorebi-bar = {
                  type = "app";
                  program = "${build.komorebi-bar}/bin/komorebi-bar";
                };
                default = {
                  type = "app";
                  program = "${build.fullBuild}/bin/komorebi";
                };
              };

              devShells.default = pkgs.mkShell {
                name = "komorebi";

                RUST_BACKTRACE = "full";

                inputsFrom = [ build.fullBuild ];

                packages = [
                  rustToolchainWithNightlyRustfmt
                  cargo-udeps

                  pkgs.cargo-deny
                  pkgs.cargo-nextest
                  pkgs.cargo-outdated
                  pkgs.jq
                  pkgs.just
                  pkgs.prettier
                  pkgs.wrangler
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
                build = mkKomorebiPackages {
                  inherit pkgs;
                };
              in
              {
                inherit (build) komorebi komorebic komorebi-bar;
                komorebi-full = build.fullBuild;
              };

            overlays.komorebi =
              final: _:
              let
                pkgs = mkPkgs final.system;
                build = mkKomorebiPackages {
                  inherit pkgs;
                };
              in
              {
                inherit (build) komorebi komorebic komorebi-bar;
                komorebi-full = build.fullBuild;
              };

            darwinModules.default = import ./nix/darwin-module.nix;
            darwinModules.komorebi = import ./nix/darwin-module.nix;
          };
        };
    in
    mkOutputs {
      inherit inputs self;
      repoRoot = ./.;
      platformDir = "";
    }
    // {
      lib.mkOutputs = mkOutputs;
    };
}
