# Nix equivalent of docs/komorebi.example.json
# This tests that users can write the exact same config in Nix
{ ... }:
{
  komorebi = {
    app_specific_configuration_path = "$HOME/.config/komorebi/applications.json";
    cross_monitor_move_behaviour = "Insert";
    default_workspace_padding = 15;
    default_container_padding = 15;
    border = true;
    border_width = 6;
    border_offset = 5;
    floating_window_aspect_ratio = "Widescreen";
    floating_layer_behaviour = "Float";
    resize_delta = 100;
    ignore_rules = [ ];
    floating_applications = [ ];
    manage_rules = [ ];
    monitors = [
      {
        workspaces = [
          {
            name = "I";
            layout = "BSP";
            initial_workspace_rules = [ ];
          }
          {
            name = "II";
            layout = "VerticalStack";
          }
          {
            name = "III";
            layout = "HorizontalStack";
          }
          {
            name = "IV";
            layout = "UltrawideVerticalStack";
          }
          {
            name = "V";
            layout = "Rows";
          }
          {
            name = "VI";
            layout = "Grid";
          }
          {
            name = "VII";
            layout = "RightMainVerticalStack";
          }
          {
            name = "VIII";
            layout = "Scrolling";
          }
        ];
      }
    ];
  };
}
