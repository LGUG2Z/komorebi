# E2E test harness for validating generated Nix options
# run with: nix build -f nix/tests/default.nix
# or via flake: nix flake check (runs as part of checks)
{
  pkgs ? import <nixpkgs> { },
}:
let
  lib = pkgs.lib;

  # recursively filter out null values and empty attrs from a config
  # this is needed because the Nix module system uses null for unset optional fields,
  # but the JSON schema expects those fields to be absent entirely
  filterNulls =
    value:
    if value == null then
      null
    else if builtins.isList value then
      map filterNulls (builtins.filter (x: x != null) value)
    else if builtins.isAttrs value then
      let
        filtered = lib.filterAttrs (_n: v: v != null) (builtins.mapAttrs (_n: v: filterNulls v) value);
      in
      filtered
    else
      value;

  evalConfig =
    optionsModule: configModule: configPath:
    let
      evaluated = lib.evalModules {
        modules = [
          optionsModule
          configModule
        ];
      };
      rawConfig = lib.getAttrFromPath configPath evaluated.config;
    in
    filterNulls rawConfig;

  komorebiOptions = import ../komorebi-options.nix;
  komorebiBarOptions = import ../komorebi-bar-options.nix;

  komorebiExample = import ./komorebi-example.nix;
  komorebiBarExample = import ./komorebi-bar-example.nix;

  configs = {
    komorebi-example = evalConfig komorebiOptions komorebiExample [ "komorebi" ];
    komorebi-bar-example = evalConfig komorebiBarOptions komorebiBarExample [ "komorebi-bar" ];
  };

  komorebiJson = pkgs.writeText "komorebi-example.json" (builtins.toJSON configs.komorebi-example);
  komorebiBarJson = pkgs.writeText "komorebi-bar-example.json" (
    builtins.toJSON configs.komorebi-bar-example
  );

  schemaPath = ../../schema.json;
  barSchemaPath = ../../schema.bar.json;

in
pkgs.runCommand "nix-options-validation"
  {
    nativeBuildInputs = [ pkgs.check-jsonschema ];
  }
  ''
    echo "Validating komorebi config against schema..."
    check-jsonschema --schemafile ${schemaPath} ${komorebiJson}

    echo "Validating komorebi-bar config against schema..."
    check-jsonschema --schemafile ${barSchemaPath} ${komorebiBarJson}

    echo "All validations passed!"
    touch $out
  ''
