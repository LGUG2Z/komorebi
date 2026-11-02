# komorebi for macOS: the native packages on Apple machines, the `macos-*` checks and the
# nix-darwin module. The Rust packages cannot be built on Linux; `nix flake check --no-build
# --all-systems` evaluates them there.
{
  inputs,
  self,
  lib,
  ...
}:
let
  lib' = import ../lib.nix { inherit inputs; };
  macos = import ./build.nix { inherit inputs self; };
  inherit (lib') mkPkgs;
in
{
  perSystem =
    { pkgs, system, ... }:
    let
      build = macos.mkKomorebiPackages { inherit pkgs; };
      # rustfmt.toml uses options only a nightly rustfmt knows
      nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
    in
    lib.optionalAttrs (builtins.elem system lib'.macosSystems) {
      allChecks = {
        macos-clippy = build.craneLib.cargoClippy build.individualCrateArgs;

        # the nightly rustfmt over the macOS files only (the source of this platform has no others), with
        # the 2024 edition of the manifests. `cargo fmt` would follow the `mod windows;` of the shared
        # entry files (komorebi-bar, komorebi-client), which the macOS source does not have.
        macos-fmt =
          pkgs.runCommand "komorebi-macos-fmt"
            {
              nativeBuildInputs = [ nightlyRustfmt ];
            }
            ''
              RUSTFMT=rustfmt bash ${lib'.repoFile "tools/format-all"} --check --root ${build.real}
              touch $out
            '';

        macos-toml-fmt = build.craneLib.taploFmt (
          build.workspaceArgs
          // {
            src = lib.sources.sourceFilesBySuffices build.real [ ".toml" ];
          }
        );

        # cargo deny sees every member of the workspace: leave out the other platforms' packages (stubs
        # here); the policy is the one deny.toml of the repository
        macos-deny = build.craneLib.cargoDeny (
          build.workspaceArgs
          // {
            inherit (build) src;
            cargoDenyExtraArgs =
              "--workspace " + lib.concatMapStringsSep " " (p: "--exclude ${p}") build.stubbedPackages;
            cargoDenyChecks = "bans licenses sources";
          }
        );

        macos-nextest = build.craneLib.cargoNextest (build.individualCrateArgs // { doCheck = true; });

        macos-nix-options-validation = import ./tests { inherit pkgs; };

        macos-darwin-module-validation = import ./tests/darwin-module.nix { inherit pkgs; };
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
    };

  flake = {
    overlays.macos =
      final: _:
      let
        build = macos.mkKomorebiPackages { pkgs = mkPkgs final.stdenv.hostPlatform.system; };
      in
      {
        inherit (build) komorebi komorebic komorebi-bar;
        komorebi-full = build.fullBuild;
      };

    darwinModules.default = import ./darwin-module.nix;
    darwinModules.komorebi = import ./darwin-module.nix;
    # the name of the root flake before the nested flakes were merged into it
    darwinModules.komorebi-for-mac = import ./darwin-module.nix;
  };
}
