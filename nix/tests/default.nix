# `nix-options-validation`: the options `just nixgen` makes from `schema.json`, evaluated against
# the example configuration, whose JSON is checked against the schema, and against the example
# file of the docs. Run with `nix build -f nix/tests/default.nix`, or as a check of the flake.
{
  pkgs ? import <nixpkgs> { },
}:
let
  lib = pkgs.lib;

  # the Nix module system uses null for an optional field that is not set, and the schema wants
  # the field to be absent
  filterNulls =
    value:
    if value == null then
      null
    else if builtins.isList value then
      map filterNulls (builtins.filter (x: x != null) value)
    else if builtins.isAttrs value then
      lib.filterAttrs (_n: v: v != null) (builtins.mapAttrs (_n: v: filterNulls v) value)
    else
      value;

  evaluated =
    (lib.evalModules {
      modules = [
        (import ../komorebi-options.nix)
        (import ./komorebi-example.nix)
      ];
    }).config.komorebi;

  evaluatedJson = pkgs.writeText "komorebi-example-from-nix.json" (
    builtins.toJSON (filterNulls evaluated)
  );

  # the file has comment lines (komorebi reads them), which the schema checker does not
  exampleJson = pkgs.runCommand "komorebi.example.json" { } ''
    grep -v '^\s*//' ${../../docs/komorebi.example.json} > $out
  '';
  schema = ../../schema.json;
in
pkgs.runCommand "nix-options-validation"
  {
    nativeBuildInputs = [
      pkgs.check-jsonschema
      pkgs.jq
    ];
  }
  ''
    echo "Validating the example, written in Nix, against the schema..."
    check-jsonschema --schemafile ${schema} ${evaluatedJson}

    echo "Validating the example file of the docs against the schema..."
    check-jsonschema --schemafile ${schema} ${exampleJson}

    echo "Checking that the two examples say the same..."
    jq -e --slurpfile example ${exampleJson} 'contains($example[0])' ${evaluatedJson} > /dev/null

    echo "All validations passed!"
    touch $out
  ''
