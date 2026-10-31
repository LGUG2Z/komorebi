{
  pkgs ? import <nixpkgs> { },
}:
let
  lib = pkgs.lib;

  evalDarwinModule =
    testConfig:
    lib.evalModules {
      modules = [
        # darwin-specific opt stubs
        (
          { ... }:
          {
            options = {
              environment.systemPackages = lib.mkOption {
                type = lib.types.listOf lib.types.package;
                default = [ ];
              };
              environment.systemPath = lib.mkOption {
                type = lib.types.str;
                default = "/usr/bin:/bin";
              };
              launchd.user.agents = lib.mkOption {
                type = lib.types.attrsOf lib.types.attrs;
                default = { };
              };
            };
          }
        )
        ../darwin-module.nix
        # test config
        (
          { ... }:
          {
            _module.args.pkgs = pkgs;
          }
          // testConfig
        )
      ];
      specialArgs = {
        inherit pkgs;
      };
    };

  # test default configs
  testDefault = evalDarwinModule {
    services.komorebi.enable = true;
    services.komorebi-bar = {
      enable = true;
      bars.main = { };
    };
  };

  # test multiple bars
  testMultiMonitor = evalDarwinModule {
    services.komorebi.enable = true;
    services.komorebi-bar = {
      enable = true;
      bars = {
        primary.config.monitor = 0;
        secondary.config.monitor = 1;
        tertiary.config.monitor = 2;
      };
    };
  };

  # test custom komorebi conf
  testCustomConfig = evalDarwinModule {
    services.komorebi = {
      enable = true;
      logLevel = "debug";
      config = {
        border = true;
        border_width = 8;
        default_workspace_padding = 20;
        default_container_padding = 20;
        monitors = [
          {
            workspaces = [
              {
                name = "main";
                layout = "BSP";
              }
              {
                name = "code";
                layout = "Columns";
              }
            ];
          }
        ];
      };
    };
  };

  # test disabled
  testDisabled = evalDarwinModule {
    services.komorebi.enable = false;
    services.komorebi-bar.enable = false;
  };

  # test applicationSpecificConfiguration enabled (default)
  testAppSpecificEnabled = evalDarwinModule {
    services.komorebi.enable = true;
  };

  # test applicationSpecificConfiguration disabled
  testAppSpecificDisabled = evalDarwinModule {
    services.komorebi = {
      enable = true;
      applicationSpecificConfiguration.enable = false;
    };
  };

  # test applicationSpecificConfiguration with multiple sources
  testAppSpecificMultipleSources = evalDarwinModule {
    services.komorebi = {
      enable = true;
      applicationSpecificConfiguration.sources = [
        {
          owner = "LGUG2Z";
          repo = "komorebi-application-specific-configuration";
          rev = "09ebdcd95780b168cecde2f2d49096a58e91365f";
          path = "applications.mac.json";
          hash = "sha256-gfYubJpVEU6vxdd/vqujQhQX8mdAEE9ImugqdIyTQKk=";
        }
        {
          owner = "LGUG2Z";
          repo = "komorebi-application-specific-configuration";
          rev = "09ebdcd95780b168cecde2f2d49096a58e91365f";
          path = "applications.mac.json";
          hash = "sha256-gfYubJpVEU6vxdd/vqujQhQX8mdAEE9ImugqdIyTQKk=";
        }
      ];
    };
  };

  # test applicationSpecificConfiguration with custom single source
  testAppSpecificCustomSource = evalDarwinModule {
    services.komorebi = {
      enable = true;
      applicationSpecificConfiguration.sources = [
        {
          owner = "someuser";
          repo = "custom-config";
          rev = "abc123";
          path = "my-apps.json";
          hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        }
      ];
    };
  };

  # test applicationSpecificConfiguration with empty sources
  testAppSpecificEmptySources = evalDarwinModule {
    services.komorebi = {
      enable = true;
      applicationSpecificConfiguration.sources = [ ];
    };
  };

  # test configFile option for komorebi
  testKomorebiConfigFile = evalDarwinModule {
    services.komorebi = {
      enable = true;
      configFile = ../../docs/komorebi.example.json;
    };
  };

  # test configFile option for komorebi-bar
  testBarConfigFile = evalDarwinModule {
    services.komorebi-bar = {
      enable = true;
      bars.main.configFile = ../../docs/komorebi.bar.example.json;
    };
  };

  assertions = [
    {
      name = "default-komorebi-enabled";
      assertion = testDefault.config.services.komorebi.enable == true;
    }
    {
      name = "default-komorebi-bar-enabled";
      assertion = testDefault.config.services.komorebi-bar.enable == true;
    }
    {
      name = "default-has-komorebi-agent";
      assertion = builtins.hasAttr "komorebi" testDefault.config.launchd.user.agents;
    }
    {
      name = "default-has-bar-agent";
      assertion = builtins.hasAttr "komorebi-bar-main" testDefault.config.launchd.user.agents;
    }
    {
      name = "multi-monitor-has-three-bars";
      assertion =
        let
          agents = testMultiMonitor.config.launchd.user.agents;
        in
        builtins.hasAttr "komorebi-bar-primary" agents
        && builtins.hasAttr "komorebi-bar-secondary" agents
        && builtins.hasAttr "komorebi-bar-tertiary" agents;
    }
    {
      name = "custom-config-log-level";
      assertion = testCustomConfig.config.services.komorebi.logLevel == "debug";
    }
    {
      name = "custom-config-border-width";
      assertion = testCustomConfig.config.services.komorebi.config.border_width == 8;
    }
    {
      name = "disabled-no-komorebi-agent";
      assertion = !(builtins.hasAttr "komorebi" testDisabled.config.launchd.user.agents);
    }
    {
      name = "disabled-no-bar-agents";
      assertion = testDisabled.config.launchd.user.agents == { };
    }
    {
      name = "app-specific-enabled-by-default";
      assertion =
        testAppSpecificEnabled.config.services.komorebi.applicationSpecificConfiguration.enable == true;
    }
    {
      name = "app-specific-has-default-source";
      assertion =
        let
          sources = testAppSpecificEnabled.config.services.komorebi.applicationSpecificConfiguration.sources;
        in
        builtins.length sources == 1 && (builtins.head sources).owner == "LGUG2Z";
    }
    {
      name = "app-specific-disabled-works";
      assertion =
        testAppSpecificDisabled.config.services.komorebi.applicationSpecificConfiguration.enable == false;
    }
    {
      name = "app-specific-multiple-sources-count";
      assertion =
        builtins.length testAppSpecificMultipleSources.config.services.komorebi.applicationSpecificConfiguration.sources
        == 2;
    }
    {
      name = "app-specific-custom-source-owner";
      assertion =
        let
          sources =
            testAppSpecificCustomSource.config.services.komorebi.applicationSpecificConfiguration.sources;
        in
        (builtins.head sources).owner == "someuser";
    }
    {
      name = "app-specific-empty-sources";
      assertion =
        testAppSpecificEmptySources.config.services.komorebi.applicationSpecificConfiguration.sources
        == [ ];
    }
    {
      name = "config-file-komorebi-sets-option";
      assertion = testKomorebiConfigFile.config.services.komorebi.configFile != null;
    }
    {
      name = "config-file-bar-sets-option";
      assertion = testBarConfigFile.config.services.komorebi-bar.bars.main.configFile != null;
    }
  ];

  failedAssertions = builtins.filter (a: !a.assertion) assertions;

  komorebiValidatedConfigPath = builtins.elemAt testKomorebiConfigFile.config.launchd.user.agents.komorebi.serviceConfig.ProgramArguments 2;
  barValidatedConfigPath = builtins.elemAt testBarConfigFile.config.launchd.user.agents.komorebi-bar-main.serviceConfig.ProgramArguments 2;

in
pkgs.runCommand "darwin-module-validation" { } ''
  ${
    if failedAssertions == [ ] then
      ''
        echo "All ${toString (builtins.length assertions)} darwin module assertions passed!"

        echo "Validating komorebi configFile against schema..."
        cat ${komorebiValidatedConfigPath} > /dev/null

        echo "Validating komorebi-bar configFile against schema..."
        cat ${barValidatedConfigPath} > /dev/null

        echo "All configFile validations passed!"
        touch $out
      ''
    else
      ''
        echo "Failed assertions:"
        ${lib.concatMapStringsSep "\n" (a: "echo '  - ${a.name}'") failedAssertions}
        exit 1
      ''
  }
''
