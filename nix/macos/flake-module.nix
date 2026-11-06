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
  # the options and the nix-darwin module are plain Nix: they take a plain `pkgs` and are evaluated on
  # every system, so that a Linux runner can check them (the ci workflow does)
  imports = [
    {
      perSystem =
        { pkgs, ... }:
        {
          allChecks = {
            macos-nix-options-validation = import ./tests { inherit pkgs; };
            macos-darwin-module-validation = import ./tests/darwin-module.nix { inherit pkgs; };
          };
        };
    }
  ];

  perSystem =
    { pkgs, system, ... }:
    let
      build = macos.mkKomorebiPackages { inherit pkgs; };
      # rustfmt.toml uses options only a nightly rustfmt knows
      nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;

      # Tart runs the macOS guests of the e2e suite (e2e/platforms/tart_macos). Its licence (Fair Source)
      # is an unfree one in nixpkgs, so it is allowed here, and only it
      tart =
        (import inputs.nixpkgs {
          inherit system;
          config.allowUnfreePredicate = pkg: lib.getName pkg == "tart";
        }).tart;

      # the scenarios of the shared e2e core against a macOS guest: the golden image is built once by
      # e2e/platforms/tart_macos/build-golden.sh, and the programs under test are this checkout's, or those
      # of another build (KOMOREBI_E2E_BINS, made by build-bins.sh)
      e2eMacos = pkgs.writeShellApplication {
        name = "e2e-macos";
        runtimeInputs = [ (pkgs.python3.withPackages (p: [ p.pillow ])) ];
        text = ''
          golden=''${KOMOREBI_E2E_GOLDEN:-komorebi-e2e-golden}
          out=''${1:?usage: e2e-macos OUT SCENARIO...}
          shift
          if [ -n "''${KOMOREBI_E2E_BINS:-}" ]; then
            bins=$KOMOREBI_E2E_BINS
            if [ -f "$bins/commit" ]; then
              KOMOREBI_E2E_COMMIT=$(cat "$bins/commit")
            fi
          else
            bins=$(mktemp -d)
            trap 'rm -rf "$bins"' EXIT
            cp ${build.komorebi}/bin/komorebi ${build.komorebic}/bin/komorebic \
              ${build.e2eBuild}/bin/komorebi-testwin "$bins"
          fi
          export KOMOREBI_E2E_TART=''${KOMOREBI_E2E_TART:-${tart}/bin/tart}
          export KOMOREBI_E2E_COMMIT=''${KOMOREBI_E2E_COMMIT:-${self.rev or "dirty"}}
          export PYTHONPATH=${lib'.repoFile "e2e/lib"}:${lib'.repoFile "e2e"}
          exec python -m platforms.tart_macos.run "$golden" "$bins" "$out" "$@"
        '';
      };
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
      };

      packages = {
        inherit (build) komorebi komorebic komorebi-bar;
        komorebi-full = build.fullBuild;
        default = build.fullBuild;
      }
      // lib.optionalAttrs (system == "aarch64-darwin") {
        komorebi-e2e-macos = build.e2eBuild;
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
      }
      // lib.optionalAttrs (system == "aarch64-darwin") {
        # nix run .#e2e-macos -- OUT tiling focus
        e2e-macos = {
          type = "app";
          program = "${e2eMacos}/bin/e2e-macos";
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
