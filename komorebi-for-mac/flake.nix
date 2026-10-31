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
      mkKomorebiPackages =
        { pkgs }:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          version = "0.1.0";

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
}
