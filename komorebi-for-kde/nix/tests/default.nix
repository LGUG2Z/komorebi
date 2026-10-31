# `nix-options-validation`: the options `just nixgen` makes from `schema.json`, evaluated against
# the example configuration, whose JSON is checked against the schema, and against the example
# file of the docs, and against the places a workspace can give the windows that open, in every
# form komorebi reads. Run with `nix build -f nix/tests/default.nix`, or as a check of the flake.
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

  # where a workspace puts the windows that open, in each form komorebi reads: a name, a number,
  # text that is a number, and a map of places to rules (whose keys are text)
  placements =
    (lib.evalModules {
      modules = [
        (import ../komorebi-options.nix)
        {
          komorebi.monitors = [
            {
              workspaces = [
                {
                  name = "name";
                  initial_window_placement_rules = "Primary";
                }
                {
                  name = "number";
                  initial_window_placement_rules = 3;
                }
                {
                  name = "text";
                  initial_window_placement_rules = "3";
                }
                {
                  name = "map";
                  initial_window_placement_rules = {
                    Primary = {
                      kind = "Exe";
                      id = "firefox";
                      matching_strategy = "Equals";
                    };
                    "2" = [
                      {
                        kind = "Title";
                        id = "notes";
                        matching_strategy = "StartsWith";
                      }
                    ];
                  };
                }
              ];
            }
          ];
        }
      ];
    }).config.komorebi;

  placementsJson = pkgs.writeText "komorebi-placements.json" (
    builtins.toJSON (filterNulls placements)
  );

  # a place that is neither a name nor a number
  nowhereJson = pkgs.writeText "komorebi-nowhere.json" (
    builtins.toJSON {
      monitors = [
        {
          workspaces = [
            {
              name = "x";
              initial_window_placement_rules = "Nowhere";
            }
          ];
        }
      ];
    }
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

    echo "Validating the places a workspace can give a new window, in every form, against the schema..."
    check-jsonschema --schemafile ${schema} ${placementsJson}
    jq -e '[.monitors[0].workspaces[].initial_window_placement_rules | type] == ["string", "number", "string", "object"]' ${placementsJson} > /dev/null

    echo "Checking that the schema refuses a place that is neither a name nor a number..."
    if check-jsonschema --schemafile ${schema} ${nowhereJson} > /dev/null 2>&1; then
      echo "the schema accepts a place that komorebi cannot read" >&2
      exit 1
    fi

    echo "All validations passed!"
    touch $out
  ''
