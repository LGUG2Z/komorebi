# Nix equivalent of docs/komorebi.bar.example.json
# This tests that users can write the exact same config in Nix
{ ... }:
{
  komorebi-bar = {
    monitor = 0;
    font_family = "JetBrainsMono Nerd Font";
    height = 30.0;

    theme = {
      palette = "Base16";
      name = "Ashes";
      accent = "Base0D";
    };

    left_widgets = [
      {
        Komorebi = {
          workspaces = {
            enable = true;
            hide_empty_workspaces = false;
          };
          layout = {
            enable = true;
          };
          focused_container = {
            enable = true;
            show_icon = true;
          };
        };
      }
    ];

    right_widgets = [
      {
        Update = {
          enable = true;
        };
      }
      {
        Storage = {
          enable = true;
        };
      }
      {
        Memory = {
          enable = true;
        };
      }
      {
        Network = {
          enable = true;
          show_activity = true;
          show_total_activity = true;
        };
      }
      {
        Date = {
          enable = true;
          format = "DayDateMonthYear";
        };
      }
      {
        Time = {
          enable = true;
          format = "TwentyFourHour";
        };
      }
    ];
  };
}
