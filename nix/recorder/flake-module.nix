# komorebi-recorder: the native packages on macOS and Linux and their checks. On Windows it is built
# with the rest of the workspace (nix/windows), and its Windows checks are the `windows-*` ones.
{
  inputs,
  self,
  lib,
  ...
}:
let
  lib' = import ../lib.nix { inherit inputs; };
  recorder = import ./build.nix { inherit inputs self; };
  inherit (lib') mkPkgs;
in
{
  perSystem =
    { pkgs, system, ... }:
    let
      build = recorder.mkRecorderPackages { inherit pkgs; };
    in
    lib.optionalAttrs (builtins.elem system (lib'.macosSystems ++ lib'.linuxSystems)) {
      allChecks = {
        # the format, the TOML and the licenses of the recorder are checked with those of the rest of
        # the workspace (`windows-fmt`, `windows-toml-fmt`, `windows-deny`)
        recorder-clippy = build.craneLib.cargoClippy (
          build.individualCrateArgs
          // {
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );

        recorder-nextest = build.craneLib.cargoNextest (build.individualCrateArgs // { doCheck = true; });
      };

      packages = {
        inherit (build) komorebi-recorder;
        komorebi-recorder-full = build.fullBuild;
      };

      apps.komorebi-recorder = {
        type = "app";
        program = "${build.komorebi-recorder}/bin/komorebi-recorder";
      };
    };

  flake.overlays.recorder =
    final: _:
    lib.optionalAttrs
      (builtins.elem final.stdenv.hostPlatform.system (lib'.macosSystems ++ lib'.linuxSystems))
      {
        inherit (recorder.mkRecorderPackages { pkgs = mkPkgs final.stdenv.hostPlatform.system; })
          komorebi-recorder
          ;
      };
}
