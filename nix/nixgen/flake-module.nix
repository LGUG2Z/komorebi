# schemars-nixgen: the package of the tool that writes the Nix options (`just nixgen-linux`,
# `just nixgen-macos`), and its checks. It is a development tool: nothing of a release has it.
{
  inputs,
  self,
  lib,
  ...
}:
let
  lib' = import ../lib.nix { inherit inputs; };
  nixgen = import ./build.nix { inherit inputs self; };
in
{
  perSystem =
    { pkgs, system, ... }:
    let
      build = nixgen.mkNixgenPackages { inherit pkgs; };
    in
    lib.optionalAttrs (builtins.elem system (lib'.macosSystems ++ lib'.linuxSystems)) {
      allChecks = {
        # the format, the TOML and the licenses of the tool are checked with those of the rest of
        # the workspace (`windows-fmt`, `windows-toml-fmt`, `cargo deny` over the workspace)
        nixgen-clippy = build.craneLib.cargoClippy (
          build.individualCrateArgs
          // {
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );

        nixgen-nextest = build.craneLib.cargoNextest (build.individualCrateArgs // { doCheck = true; });
      };

      packages = {
        inherit (build) schemars-nixgen;
      };

      apps.schemars-nixgen = {
        type = "app";
        program = "${build.schemars-nixgen}/bin/schemars-nixgen";
      };
    };
}
