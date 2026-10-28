{
  tier = "fast";
  kind = "session";
  timeoutMinutes = 10;
  # the unit starts the daemon with this file, as `--config`: two named workspaces, and a padding
  # that moves the tile of the first window away from the edge of the screen
  node =
    { pkgs, ... }:
    {
      services.komorebi.configFile = pkgs.writeText "komorebi.json" ''
        {
          "default_workspace_padding": 40,
          "default_container_padding": 0,
          "monitors": [
            {
              "workspaces": [
                { "name": "web", "layout": "BSP" },
                { "name": "code", "layout": "Columns" }
              ]
            }
          ]
        }
      '';
    };
}
