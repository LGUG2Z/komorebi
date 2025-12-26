{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
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
      nixpkgs,
      flake-parts,
      crane,
      rust-overlay,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        {
          config,
          system,
          ...
        }:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

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

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || (builtins.match ".*\\.css$" path != null)
              || (builtins.match ".*\\.html$" path != null)
              || (builtins.match ".*\\.md$" path != null)
              || (builtins.match ".*\\.json$" path != null);
          };

          commonArgs = {
            inherit src;
            strictDeps = true;
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          schemars-docgen = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );
        in
        {
          checks = {
            clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
              }
            );
            tests = craneLib.cargoTest (
              commonArgs
              // {
                inherit cargoArtifacts;
              }
            );
          };

          packages = {
            inherit schemars-docgen;
            default = schemars-docgen;
          };

          devShells.default = pkgs.mkShell {
            inputsFrom = [ schemars-docgen ];
            packages = [
              rustToolchainWithNightlyRustfmt
              cargo-udeps
              pkgs.cargo-outdated
              pkgs.bacon
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

          treefmt = {
            projectRootFile = "flake.nix";
            programs = {
              rustfmt = {
                enable = true;
                package = nightlyRustfmt;
              };
              taplo.enable = true;
              nixfmt.enable = true;
              deadnix.enable = true;
            };
          };
        };
    };
}
