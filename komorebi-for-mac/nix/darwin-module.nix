{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.komorebi;
  cfgBar = config.services.komorebi-bar;

  jsonFormat = pkgs.formats.json { };

  komorebiSchemaPath = ../schema.json;
  komorebiBarSchemaPath = ../schema.bar.json;

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

  # validate a json file against the flake's version of a schema, returning the validated file path
  # strips comment lines before validation (common in configs for editor support)
  validateJsonFile =
    {
      name,
      jsonFile,
      schemaFile,
    }:
    pkgs.runCommand "validated-${name}" { nativeBuildInputs = [ pkgs.check-jsonschema ]; } ''
      echo "Validating ${name} against schema..."
      # Strip // comment lines before validation
      grep -v '^\s*//' ${jsonFile} > clean.json
      check-jsonschema --schemafile ${schemaFile} clean.json
      cp clean.json $out
    '';

  komorebiOptionsModule = import ./komorebi-options.nix { inherit lib; };
  komorebiBarOptionsModule = import ./komorebi-bar-options.nix { inherit lib; };

  defaultKomorebiConfig = (import ./tests/komorebi-example.nix { }).komorebi;
  defaultKomorebiBarConfig = (import ./tests/komorebi-bar-example.nix { }).komorebi-bar;

  # fetch application-specific configuration from remote sources
  fetchAppSpecificSource =
    source:
    pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/${source.owner}/${source.repo}/${source.rev}/${source.path}";
      hash = source.hash;
    };

  applicationSpecificConfigFiles = map fetchAppSpecificSource cfg.applicationSpecificConfiguration.sources;

  # build app_specific_configuration_path value:
  # - single source: use single path (string)
  # - multiple sources: use array of paths
  appSpecificPaths =
    if builtins.length applicationSpecificConfigFiles == 1 then
      "${builtins.head applicationSpecificConfigFiles}"
    else
      map (f: "${f}") applicationSpecificConfigFiles;

  # build final komorebi config
  # if applicationSpecificConfiguration is enabled and has sources: set app_specific_configuration_path
  # if disabled or empty sources: remove app_specific_configuration_path entirely (handles default config with $HOME path)
  shouldUseAppSpecific =
    cfg.applicationSpecificConfiguration.enable && applicationSpecificConfigFiles != [ ];

  baseKomorebiConfig =
    if shouldUseAppSpecific then
      cfg.config
    else
      removeAttrs cfg.config [ "app_specific_configuration_path" ];

  finalKomorebiConfig = filterNulls (
    baseKomorebiConfig
    // lib.optionalAttrs shouldUseAppSpecific {
      app_specific_configuration_path = appSpecificPaths;
    }
  );

  generatedKomorebiConfigFile = jsonFormat.generate "komorebi.json" finalKomorebiConfig;

  # final komorebi config file - either user-provided (validated) or generated from Nix options
  komorebiConfigFile =
    if cfg.configFile != null then
      validateJsonFile {
        name = "komorebi.json";
        jsonFile = cfg.configFile;
        schemaFile = komorebiSchemaPath;
      }
    else
      generatedKomorebiConfigFile;

  mkGeneratedBarConfigFile =
    name: barCfg: jsonFormat.generate "komorebi-bar-${name}.json" (filterNulls barCfg.config);

  # final bar config - either user-provided (validated) or generated from Nix options
  mkBarConfigFile =
    name: barCfg:
    if barCfg.configFile != null then
      validateJsonFile {
        name = "komorebi-bar-${name}.json";
        jsonFile = barCfg.configFile;
        schemaFile = komorebiBarSchemaPath;
      }
    else
      mkGeneratedBarConfigFile name barCfg;

in
{
  options.services.komorebi = {
    enable = lib.mkEnableOption "komorebi for Mac";
    package = lib.mkPackageOption pkgs "komorebi" { };

    logLevel = lib.mkOption {
      type = lib.types.enum [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      default = "info";
      description = "Log verbosity level";
    };

    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Path to a komorebi JSON configuration file.
        If specified, this file will be validated against the schema and used instead of the `config` option.
      '';
      example = lib.literalExpression "./komorebi.json";
    };

    config = lib.mkOption {
      type = komorebiOptionsModule.options.komorebi.type;
      default = defaultKomorebiConfig;
      description = ''
        komorebi configuration (validated against schema). Defaults to example config.
        Ignored if `configFile` is set.
      '';
    };

    applicationSpecificConfiguration = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether to use application-specific configuration sources.";
      };

      sources = lib.mkOption {
        type = lib.types.listOf (
          lib.types.submodule {
            options = {
              owner = lib.mkOption {
                type = lib.types.str;
                description = "GitHub repository owner.";
                example = "LGUG2Z";
              };
              repo = lib.mkOption {
                type = lib.types.str;
                description = "GitHub repository name.";
                example = "komorebi-application-specific-configuration";
              };
              rev = lib.mkOption {
                type = lib.types.str;
                description = "Git revision (commit hash, tag, or branch).";
                example = "09ebdcd95780b168cecde2f2d49096a58e91365f";
              };
              path = lib.mkOption {
                type = lib.types.str;
                default = "applications.mac.json";
                description = "Path to the JSON file within the repository.";
              };
              hash = lib.mkOption {
                type = lib.types.str;
                description = "SHA256 hash of the file for reproducibility.";
                example = "sha256-gfYubJpVEU6vxdd/vqujQhQX8mdAEE9ImugqdIyTQKk=";
              };
            };
          }
        );
        default = [
          {
            owner = "LGUG2Z";
            repo = "komorebi-application-specific-configuration";
            rev = "09ebdcd95780b168cecde2f2d49096a58e91365f";
            path = "applications.mac.json";
            hash = "sha256-gfYubJpVEU6vxdd/vqujQhQX8mdAEE9ImugqdIyTQKk=";
          }
        ];
        description = ''
          List of remote sources for application-specific configuration files.
          Each source is fetched from GitHub and combined into the final config.
        '';
        example = lib.literalExpression ''
          [
            {
              owner = "LGUG2Z";
              repo = "komorebi-application-specific-configuration";
              rev = "09ebdcd95780b168cecde2f2d49096a58e91365f";
              path = "applications.mac.json";
              hash = "sha256-gfYubJpVEU6vxdd/vqujQhQX8mdAEE9ImugqdIyTQKk=";
            }
            {
              owner = "myuser";
              repo = "my-komorebi-overrides";
              rev = "abc123...";
              path = "overrides.json";
              hash = "sha256-...";
            }
          ]
        '';
      };
    };
  };

  options.services.komorebi-bar = {
    enable = lib.mkEnableOption "komorebi-bar status bar(s)";
    package = lib.mkPackageOption pkgs "komorebi-bar" { };

    bars = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            configFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = ''
                Path to a komorebi-bar JSON configuration file.
                If specified, this file will be validated against the schema and used instead of the `config` option.
              '';
              example = lib.literalExpression "./komorebi.bar.json";
            };

            config = lib.mkOption {
              type = komorebiBarOptionsModule.options.komorebi-bar.type;
              default = defaultKomorebiBarConfig;
              description = ''
                komorebi-bar configuration for this bar instance.
                Ignored if `configFile` is set.
              '';
            };
          };
        }
      );
      default = { };
      example = lib.literalExpression ''
        {
          main = {
            config = {
              monitor = 0;
              left_widgets = [ ... ];
              right_widgets = [ ... ];
            };
          };
          secondary = {
            # Use a JSON file for this bar
            configFile = ./my-secondary-bar.json;
          };
        }
      '';
      description = "Named komorebi-bar instances, one per monitor";
    };
  };

  config = lib.mkMerge [
    # komorebi
    (lib.mkIf cfg.enable {
      environment.systemPackages = [
        cfg.package
        pkgs.komorebic
      ];

      launchd.user.agents.komorebi = {
        serviceConfig.ProgramArguments = [
          "${cfg.package}/bin/komorebi"
          "--config"
          "${komorebiConfigFile}"
          "--log-level"
          cfg.logLevel
        ];
        serviceConfig.KeepAlive = true;
        serviceConfig.RunAtLoad = true;
        serviceConfig.EnvironmentVariables = {
          PATH = "${cfg.package}/bin:${config.environment.systemPath}";
        };
      };
    })

    # komorebi-bar
    (lib.mkIf cfgBar.enable {
      environment.systemPackages = [ cfgBar.package ];

      launchd.user.agents = lib.mapAttrs' (
        name: barCfg:
        lib.nameValuePair "komorebi-bar-${name}" {
          serviceConfig.ProgramArguments = [
            "${cfgBar.package}/bin/komorebi-bar"
            "--config"
            "${mkBarConfigFile name barCfg}"
          ];
          serviceConfig.KeepAlive = true;
          serviceConfig.RunAtLoad = true;
        }
      ) cfgBar.bars;
    })
  ];
}
