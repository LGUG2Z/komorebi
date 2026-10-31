{
  tier = "fast";
  kind = "session";
  timeoutMinutes = 10;
  # the module writes this as a komorebi.json, checks it against the schema, and the unit starts
  # the daemon with it, as `--config`: two named workspaces, and a padding that moves the tile of
  # the first window away from the edge of the screen. No border: the module installs the effect
  # that draws one, and a border that is drawn takes room around the window, which the test would
  # have to add to the padding (the look suite measures that)
  node.services.komorebi.config = {
    border = false;
    default_workspace_padding = 40;
    default_container_padding = 0;
    monitors = [
      {
        workspaces = [
          {
            name = "web";
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
}
