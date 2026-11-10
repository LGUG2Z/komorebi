# schemars-docgen: the package of the tool that writes the reference pages of the docs site (`just docgen`),
# and its checks. It is a development tool: nothing of a release has it.
{
  inputs,
  lib,
  ...
}:
let
  lib' = import ../lib.nix { inherit inputs; };
  docgen = import ./build.nix { inherit inputs; };
in
{
  perSystem =
    { pkgs, system, ... }:
    let
      build = docgen.mkDocgenPackages { inherit pkgs; };
    in
    lib.optionalAttrs (builtins.elem system (lib'.macosSystems ++ lib'.linuxSystems)) {
      allChecks = {
        # the format, the TOML and the licenses of the tool are checked with those of the rest of
        # the workspace (`windows-fmt`, `windows-toml-fmt`, `cargo deny` over the workspace)
        docgen-clippy = build.craneLib.cargoClippy (
          build.individualCrateArgs
          // {
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );

        docgen-nextest = build.craneLib.cargoNextest (build.individualCrateArgs // { doCheck = true; });
      };

      packages = {
        inherit (build) schemars-docgen;
      };

      apps.schemars-docgen = {
        type = "app";
        program = "${build.schemars-docgen}/bin/schemars-docgen";
      };
    };
}
