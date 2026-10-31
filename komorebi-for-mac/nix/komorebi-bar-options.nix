{ lib, ... }:
let
  applicationsDisplayFormat = (
    lib.types.enum [
      "Icon"
      "Text"
      "IconAndText"
    ]
  );
  base16Value = (
    lib.types.enum [
      "Base00"
      "Base01"
      "Base02"
      "Base03"
      "Base04"
      "Base05"
      "Base06"
      "Base07"
      "Base08"
      "Base09"
      "Base0A"
      "Base0B"
      "Base0C"
      "Base0D"
      "Base0E"
      "Base0F"
    ]
  );
  catppuccinValue = (
    lib.types.enum [
      "Rosewater"
      "Flamingo"
      "Pink"
      "Mauve"
      "Red"
      "Maroon"
      "Peach"
      "Yellow"
      "Green"
      "Teal"
      "Sky"
      "Sapphire"
      "Blue"
      "Lavender"
      "Text"
      "Subtext1"
      "Subtext0"
      "Overlay2"
      "Overlay1"
      "Overlay0"
      "Surface2"
      "Surface1"
      "Surface0"
      "Base"
      "Mantle"
      "Crust"
    ]
  );
  colour = (
    lib.types.oneOf [
      (lib.types.submodule {
        options = {
          b = lib.mkOption {
            type = lib.types.int;
            description = "Blue";
          };
          g = lib.mkOption {
            type = lib.types.int;
            description = "Green";
          };
          r = lib.mkOption {
            type = lib.types.int;
            description = "Red";
          };
        };
      })
      lib.types.str
    ]
  );
  displayFormat = (
    lib.types.enum [
      "Icon"
      "Text"
      "TextAndIconOnSelected"
      "IconAndText"
      "IconAndTextOnSelected"
    ]
  );
  groupedSpacingOptions = (
    lib.types.oneOf [
      lib.types.number
      (lib.types.listOf lib.types.number)
    ]
  );
  labelPrefix = (
    lib.types.enum [
      "None"
      "Icon"
      "Text"
      "IconAndText"
    ]
  );
  position = (
    lib.types.submodule {
      options = {
        x = lib.mkOption {
          type = lib.types.number;
          description = "X coordinate";
        };
        y = lib.mkOption {
          type = lib.types.number;
          description = "Y coordinate";
        };
      };
    }
  );
  spacingKind = (
    lib.types.oneOf [
      lib.types.number
      (lib.types.submodule {
        options = {
          bottom = lib.mkOption {
            type = lib.types.number;
            description = "Spacing for the bottom";
          };
          left = lib.mkOption {
            type = lib.types.number;
            description = "Spacing for the left";
          };
          right = lib.mkOption {
            type = lib.types.number;
            description = "Spacing for the right";
          };
          top = lib.mkOption {
            type = lib.types.number;
            description = "Spacing for the top";
          };
        };
      })
      (lib.types.submodule {
        options = {
          horizontal = lib.mkOption {
            type = (lib.types.nullOr groupedSpacingOptions);
            default = null;
            description = "Horizontal grouped spacing";
          };
          vertical = lib.mkOption {
            type = (lib.types.nullOr groupedSpacingOptions);
            default = null;
            description = "Vertical grouped spacing";
          };
        };
      })
    ]
  );
  widgetConfig = (
    lib.types.submodule {
      options = {
        Applications = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  display = lib.mkOption {
                    type = (lib.types.nullOr applicationsDisplayFormat);
                    default = null;
                    description = "Default display format for all applications (optional).\nCould be overridden per application. Defaults to `Icon`.";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enables or disables the applications widget.";
                  };
                  items = lib.mkOption {
                    type = (
                      lib.types.listOf (
                        lib.types.submodule {
                          options = {
                            command = lib.mkOption {
                              type = lib.types.str;
                              description = "Command to execute (e.g. path to the application or shell command).";
                            };
                            display = lib.mkOption {
                              type = (lib.types.nullOr applicationsDisplayFormat);
                              default = null;
                              description = "Display format for this application button (optional). Overrides global format if set.";
                            };
                            enable = lib.mkOption {
                              type = (lib.types.nullOr lib.types.bool);
                              default = null;
                              description = "Whether to enable this application button (optional).\nInherits from the global `Applications` setting if omitted.";
                            };
                            icon = lib.mkOption {
                              type = (lib.types.nullOr lib.types.str);
                              default = null;
                              description = "Optional icon: a path to an image or a text-based glyph (e.g., from Nerd Fonts).\nIf not set, and if the `command` is a path to an executable, an icon might be extracted from it.\nNote: glyphs require a compatible `font_family`.";
                            };
                            name = lib.mkOption {
                              type = lib.types.str;
                              description = "Display name of the application.";
                            };
                            show_command_on_hover = lib.mkOption {
                              type = (lib.types.nullOr lib.types.bool);
                              default = null;
                              description = "Whether to show the launch command on hover (optional).\nInherits from the global `Applications` setting if omitted.";
                            };
                          };
                        }
                      )
                    );
                    description = "List of configured applications to display.";
                  };
                  show_command_on_hover = lib.mkOption {
                    type = (lib.types.nullOr lib.types.bool);
                    default = null;
                    description = "Whether to show the launch command on hover (optional).\nCould be overridden per application. Defaults to `false` if not set.";
                  };
                  spacing = lib.mkOption {
                    type = (lib.types.nullOr lib.types.number);
                    default = null;
                    description = "Horizontal spacing between application buttons.";
                  };
                };
              }
            )
          );
          default = null;
        };
        Battery = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  auto_select_under = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = null;
                    description = "Select when the current percentage is under this value [[1-100]]";
                  };
                  data_refresh_interval = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 10;
                    description = "Data refresh interval in seconds";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Battery widget";
                  };
                  hide_on_full_charge = lib.mkOption {
                    type = (lib.types.nullOr lib.types.bool);
                    default = null;
                    description = "Hide the widget if the battery is at full charge";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                };
              }
            )
          );
          default = null;
        };
        Cpu = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  auto_select_over = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = null;
                    description = "Select when the current percentage is over this value [[1-100]]";
                  };
                  data_refresh_interval = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 10;
                    description = "Data refresh interval in seconds";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Cpu widget";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                };
              }
            )
          );
          default = null;
        };
        Date = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Date widget";
                  };
                  format = lib.mkOption {
                    type = (
                      lib.types.oneOf [
                        (lib.types.enum [
                          "MonthDateYear"
                          "YearMonthDate"
                          "DateMonthYear"
                          "DayDateMonthYear"
                        ])
                        (lib.types.submodule {
                          options = {
                            Custom = lib.mkOption {
                              type = (lib.types.nullOr lib.types.str);
                              default = null;
                            };
                            CustomModifiers = lib.mkOption {
                              type = (
                                lib.types.nullOr (
                                  lib.types.submodule {
                                    options = {
                                      format = lib.mkOption {
                                        type = lib.types.str;
                                        description = "Custom format (https://docs.rs/chrono/latest/chrono/format/strftime/index.html)";
                                      };
                                      modifiers = lib.mkOption {
                                        type = (lib.types.attrsOf lib.types.int);
                                        description = "Additive modifiers for integer format specifiers (e.g. { \"%U\": 1 } to increment the zero-indexed week number by 1)";
                                      };
                                    };
                                  }
                                )
                              );
                              default = null;
                            };
                          };
                        })
                      ]
                    );
                    description = "Set the Date format";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                  timezone = lib.mkOption {
                    type = (lib.types.nullOr lib.types.str);
                    default = null;
                    description = "TimeZone (https://docs.rs/chrono-tz/latest/chrono_tz/enum.Tz.html)\n\nUse a custom format to display additional information, i.e.:\n```json\n{\n    \"Date\": {\n        \"enable\": true,\n        \"format\": { \"Custom\": \"%D %Z (Tokyo)\" },\n        \"timezone\": \"Asia/Tokyo\"\n     }\n}\n```";
                  };
                };
              }
            )
          );
          default = null;
        };
        Komorebi = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  configuration_switcher = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            configurations = lib.mkOption {
                              type = (lib.types.attrsOf lib.types.str);
                              description = "A map of display friendly name => path to configuration.json";
                            };
                            enable = lib.mkOption {
                              type = lib.types.bool;
                              description = "Enable the Komorebi Configurations widget";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Configure the Configuration Switcher widget";
                  };
                  focused_container = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            display = lib.mkOption {
                              type = (lib.types.nullOr displayFormat);
                              default = null;
                              description = "Display format of the currently focused container";
                            };
                            enable = lib.mkOption {
                              type = lib.types.bool;
                              description = "Enable the Komorebi Focused Container widget";
                            };
                            show_icon = lib.mkOption {
                              type = (lib.types.nullOr lib.types.bool);
                              default = null;
                              description = "DEPRECATED: use 'display' instead (Show the icon of the currently focused container)";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Configure the Focused Container widget";
                  };
                  layout = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            display = lib.mkOption {
                              type = (lib.types.nullOr displayFormat);
                              default = null;
                              description = "Display format of the current layout";
                            };
                            enable = lib.mkOption {
                              type = lib.types.bool;
                              description = "Enable the Komorebi Layout widget";
                            };
                            options = lib.mkOption {
                              type = (
                                lib.types.nullOr (
                                  lib.types.listOf (
                                    lib.types.oneOf [
                                      (lib.types.enum [
                                        "Monocle"
                                        "Floating"
                                        "Paused"
                                        "Custom"
                                      ])
                                      (lib.types.submodule {
                                        options = {
                                          Default = lib.mkOption {
                                            type = (
                                              lib.types.nullOr (
                                                lib.types.enum [
                                                  "BSP"
                                                  "Columns"
                                                  "Rows"
                                                  "VerticalStack"
                                                  "HorizontalStack"
                                                  "UltrawideVerticalStack"
                                                  "Grid"
                                                  "RightMainVerticalStack"
                                                  "Scrolling"
                                                ]
                                              )
                                            );
                                            default = null;
                                          };
                                        };
                                      })
                                    ]
                                  )
                                )
                              );
                              default = null;
                              description = "List of layout options";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Configure the Layout widget";
                  };
                  locked_container = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            display = lib.mkOption {
                              type = (lib.types.nullOr displayFormat);
                              default = null;
                              description = "Display format of the current locked state";
                            };
                            enable = lib.mkOption {
                              type = lib.types.bool;
                              description = "Enable the Komorebi Locked Container widget";
                            };
                            show_when_unlocked = lib.mkOption {
                              type = (lib.types.nullOr lib.types.bool);
                              default = null;
                              description = "Show the widget event if the layer is unlocked";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Configure the Locked Container widget";
                  };
                  workspace_layer = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            display = lib.mkOption {
                              type = (lib.types.nullOr displayFormat);
                              default = null;
                              description = "Display format of the current layer";
                            };
                            enable = lib.mkOption {
                              type = lib.types.bool;
                              description = "Enable the Komorebi Workspace Layer widget";
                            };
                            show_when_tiling = lib.mkOption {
                              type = (lib.types.nullOr lib.types.bool);
                              default = null;
                              description = "Show the widget event if the layer is Tiling";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Configure the Workspace Layer widget";
                  };
                  workspaces = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            display = lib.mkOption {
                              type = (
                                lib.types.nullOr (
                                  lib.types.oneOf [
                                    lib.types.str
                                    lib.types.str
                                    lib.types.str
                                    displayFormat
                                  ]
                                )
                              );
                              default = null;
                              description = "Display format of the workspace";
                            };
                            enable = lib.mkOption {
                              type = lib.types.bool;
                              description = "Enable the Komorebi Workspaces widget";
                            };
                            hide_empty_workspaces = lib.mkOption {
                              type = lib.types.bool;
                              description = "Hide workspaces without any windows";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Configure the Workspaces widget";
                  };
                };
              }
            )
          );
          default = null;
        };
        Memory = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  auto_select_over = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = null;
                    description = "Select when the current percentage is over this value [[1-100]]";
                  };
                  data_refresh_interval = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 10;
                    description = "Data refresh interval in seconds";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Memory widget";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                };
              }
            )
          );
          default = null;
        };
        Network = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  activity_left_padding = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = null;
                    description = "Characters to reserve for received and transmitted activity";
                  };
                  auto_select = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            received_over = lib.mkOption {
                              type = (lib.types.nullOr lib.types.int);
                              default = null;
                              description = "Select the received data when it's over this value";
                            };
                            total_received_over = lib.mkOption {
                              type = (lib.types.nullOr lib.types.int);
                              default = null;
                              description = "Select the total received data when it's over this value";
                            };
                            total_transmitted_over = lib.mkOption {
                              type = (lib.types.nullOr lib.types.int);
                              default = null;
                              description = "Select the total transmitted data when it's over this value";
                            };
                            transmitted_over = lib.mkOption {
                              type = (lib.types.nullOr lib.types.int);
                              default = null;
                              description = "Select the transmitted data when it's over this value";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                    description = "Select when the value is over a limit (1MiB is 1048576 bytes (1024*1024))";
                  };
                  data_refresh_interval = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 10;
                    description = "Data refresh interval in seconds";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Network widget";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                  show_activity = lib.mkOption {
                    type = lib.types.bool;
                    description = "Show received and transmitted activity";
                  };
                  show_default_interface = lib.mkOption {
                    type = (lib.types.nullOr lib.types.bool);
                    default = null;
                    description = "Show default interface";
                  };
                  show_total_activity = lib.mkOption {
                    type = lib.types.bool;
                    description = "Show total received and transmitted activity";
                  };
                };
              }
            )
          );
          default = null;
        };
        Storage = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  auto_hide_under = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = null;
                    description = "Hide when the current percentage is under this value [[1-100]]";
                  };
                  auto_select_over = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = null;
                    description = "Select when the current percentage is over this value [[1-100]]";
                  };
                  data_refresh_interval = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 10;
                    description = "Data refresh interval in seconds";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Storage widget";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                  show_read_only_disks = lib.mkOption {
                    type = (lib.types.nullOr lib.types.bool);
                    default = false;
                    description = "Show disks that are read only";
                  };
                  show_removable_disks = lib.mkOption {
                    type = (lib.types.nullOr lib.types.bool);
                    default = true;
                    description = "Show removable disks";
                  };
                  storage_display_name = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.enum [
                          "Mount"
                          "Name"
                          "MountAndName"
                          "NameAndMount"
                        ]
                      )
                    );
                    default = null;
                    description = "Storage display name";
                  };
                };
              }
            )
          );
          default = null;
        };
        Time = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  changing_icon = lib.mkOption {
                    type = (lib.types.nullOr lib.types.bool);
                    default = false;
                    description = "Change the icon depending on the time. The default icon is used between 8:30 and 12:00";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Time widget";
                  };
                  format = lib.mkOption {
                    type = (
                      lib.types.oneOf [
                        (lib.types.enum [
                          "TwelveHour"
                          "TwelveHourWithoutSeconds"
                          "TwentyFourHour"
                          "TwentyFourHourWithoutSeconds"
                          "BinaryCircle"
                          "BinaryRectangle"
                        ])
                        (lib.types.submodule {
                          options = {
                            Custom = lib.mkOption {
                              type = (lib.types.nullOr lib.types.str);
                              default = null;
                            };
                          };
                        })
                      ]
                    );
                    description = "Set the Time format";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                  timezone = lib.mkOption {
                    type = (lib.types.nullOr lib.types.str);
                    default = null;
                    description = "TimeZone (https://docs.rs/chrono-tz/latest/chrono_tz/enum.Tz.html)\n\nUse a custom format to display additional information, i.e.:\n```json\n{\n    \"Time\": {\n        \"enable\": true,\n        \"format\": { \"Custom\": \"%T %Z (Tokyo)\" },\n        \"timezone\": \"Asia/Tokyo\"\n     }\n}\n```";
                  };
                };
              }
            )
          );
          default = null;
        };
        Update = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  data_refresh_interval = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 12;
                    description = "Data refresh interval in hours";
                  };
                  enable = lib.mkOption {
                    type = lib.types.bool;
                    description = "Enable the Update widget";
                  };
                  label_prefix = lib.mkOption {
                    type = (lib.types.nullOr labelPrefix);
                    default = null;
                    description = "Display label prefix";
                  };
                };
              }
            )
          );
          default = null;
        };
      };
    }
  );
in
{
  options.komorebi-bar = lib.mkOption {
    type = lib.types.submodule {
      options = {
        center_widgets = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf widgetConfig));
          default = null;
          description = "Center widgets (ordered left-to-right)";
        };
        font_family = lib.mkOption {
          type = (lib.types.nullOr lib.types.str);
          default = null;
          description = "Font family";
        };
        font_size = lib.mkOption {
          type = (lib.types.nullOr lib.types.number);
          default = 12.5;
          description = "Font size";
        };
        frame = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  inner_margin = lib.mkOption {
                    type = position;
                    description = "Margin inside the painted frame";
                  };
                };
              }
            )
          );
          default = null;
          description = "Frame options (see: https://docs.rs/egui/latest/egui/containers/frame/struct.Frame.html)";
        };
        grouping = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  kind = lib.mkOption {
                    type = (
                      lib.types.enum [
                        "None"
                        "Bar"
                        "Alignment"
                        "Widget"
                      ]
                    );
                  };
                };
              }
            )
          );
          default = null;
          description = "Visual grouping for widgets";
        };
        height = lib.mkOption {
          type = (lib.types.nullOr lib.types.number);
          default = 50;
          description = "Bar height";
        };
        icon_scale = lib.mkOption {
          type = (lib.types.nullOr lib.types.number);
          default = 1.4;
          description = "Scale of the icons relative to the font_size [[1.0-2.0]]";
        };
        left_widgets = lib.mkOption {
          type = (lib.types.listOf widgetConfig);
          description = "Left side widgets (ordered left-to-right)";
        };
        margin = lib.mkOption {
          type = (lib.types.nullOr spacingKind);
          default = null;
          description = "Bar margin. Use one value for all sides or use a grouped margin for horizontal and/or\nvertical definition which can each take a single value for a symmetric margin or two\nvalues for each side, i.e.:\n```json\n\"margin\": {\n    \"horizontal\": 10.0\n}\n```\nor:\n```json\n\"margin\": {\n    \"vertical\": [top, bottom]\n}\n```\nYou can also set individual margin on each side like this:\n```json\n\"margin\": {\n    \"top\": 10.0,\n    \"bottom\": 10.0,\n    \"left\": 10.0,\n    \"right\": 10.0\n}\n```";
        };
        max_label_width = lib.mkOption {
          type = (lib.types.nullOr lib.types.number);
          default = 400;
          description = "Max label width before text truncation";
        };
        monitor = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.oneOf [
                lib.types.int
                (lib.types.submodule {
                  options = {
                    index = lib.mkOption {
                      type = lib.types.int;
                      description = "Komorebi monitor index of the monitor on which to render the bar";
                    };
                    work_area_offset = lib.mkOption {
                      type = (
                        lib.types.nullOr (
                          lib.types.submodule {
                            options = {
                              bottom = lib.mkOption {
                                type = lib.types.int;
                                description = "Height of the rectangle (from the top point)";
                              };
                              left = lib.mkOption {
                                type = lib.types.int;
                                description = "Left point of the rectangle";
                              };
                              right = lib.mkOption {
                                type = lib.types.int;
                                description = "Width of the recentangle (from the left point)";
                              };
                              top = lib.mkOption {
                                type = lib.types.int;
                                description = "Top point of the rectangle";
                              };
                            };
                          }
                        )
                      );
                      default = null;
                      description = "Automatically apply a work area offset for this monitor to accommodate the bar";
                    };
                  };
                })
              ]
            )
          );
          default = 0;
          description = "The monitor index or the full monitor options";
        };
        padding = lib.mkOption {
          type = (lib.types.nullOr spacingKind);
          default = null;
          description = "Bar padding. Use one value for all sides or use a grouped padding for horizontal and/or\nvertical definition which can each take a single value for a symmetric padding or two\nvalues for each side, i.e.:\n```json\n\"padding\": {\n    \"horizontal\": 10.0\n}\n```\nor:\n```json\n\"padding\": {\n    \"horizontal\": [left, right]\n}\n```\nYou can also set individual padding on each side like this:\n```json\n\"padding\": {\n    \"top\": 10.0,\n    \"bottom\": 10.0,\n    \"left\": 10.0,\n    \"right\": 10.0\n}\n```";
        };
        position = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  end = lib.mkOption {
                    type = (lib.types.nullOr position);
                    default = null;
                    description = "The desired size of the bar from the starting position (usually monitor width x desired height)";
                  };
                  start = lib.mkOption {
                    type = (lib.types.nullOr position);
                    default = null;
                    description = "The desired starting position of the bar (0,0 = top left of the screen)";
                  };
                };
              }
            )
          );
          default = null;
          description = "Bar positioning options";
        };
        right_widgets = lib.mkOption {
          type = (lib.types.listOf widgetConfig);
          description = "Right side widgets (ordered left-to-right)";
        };
        theme = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  palette = lib.mkOption {
                    type = (
                      lib.types.enum [
                        "Catppuccin"
                        "Base16"
                        "Custom"
                      ]
                    );
                  };
                  auto_select_text = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.oneOf [
                          (lib.types.nullOr catppuccinValue)
                          (lib.types.nullOr base16Value)
                        ]
                      )
                    );
                    default = null;
                  };
                  name = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.oneOf [
                          (lib.types.enum [
                            "Frappe"
                            "Latte"
                            "Macchiato"
                            "Mocha"
                          ])
                          (lib.types.enum [
                            "3024"
                            "Apathy"
                            "Apprentice"
                            "Ashes"
                            "AtelierCaveLight"
                            "AtelierCave"
                            "AtelierDuneLight"
                            "AtelierDune"
                            "AtelierEstuaryLight"
                            "AtelierEstuary"
                            "AtelierForestLight"
                            "AtelierForest"
                            "AtelierHeathLight"
                            "AtelierHeath"
                            "AtelierLakesideLight"
                            "AtelierLakeside"
                            "AtelierPlateauLight"
                            "AtelierPlateau"
                            "AtelierSavannaLight"
                            "AtelierSavanna"
                            "AtelierSeasideLight"
                            "AtelierSeaside"
                            "AtelierSulphurpoolLight"
                            "AtelierSulphurpool"
                            "Atlas"
                            "AyuDark"
                            "AyuLight"
                            "AyuMirage"
                            "Aztec"
                            "Bespin"
                            "BlackMetalBathory"
                            "BlackMetalBurzum"
                            "BlackMetalDarkFuneral"
                            "BlackMetalGorgoroth"
                            "BlackMetalImmortal"
                            "BlackMetalKhold"
                            "BlackMetalMarduk"
                            "BlackMetalMayhem"
                            "BlackMetalNile"
                            "BlackMetalVenom"
                            "BlackMetal"
                            "Blueforest"
                            "Blueish"
                            "Brewer"
                            "Bright"
                            "Brogrammer"
                            "BrushtreesDark"
                            "Brushtrees"
                            "Caroline"
                            "CatppuccinFrappe"
                            "CatppuccinLatte"
                            "CatppuccinMacchiato"
                            "CatppuccinMocha"
                            "Chalk"
                            "Circus"
                            "ClassicDark"
                            "ClassicLight"
                            "Codeschool"
                            "Colors"
                            "Cupcake"
                            "Cupertino"
                            "DaOneBlack"
                            "DaOneGray"
                            "DaOneOcean"
                            "DaOnePaper"
                            "DaOneSea"
                            "DaOneWhite"
                            "DanqingLight"
                            "Danqing"
                            "Darcula"
                            "Darkmoss"
                            "Darktooth"
                            "Darkviolet"
                            "Decaf"
                            "DefaultDark"
                            "DefaultLight"
                            "Dirtysea"
                            "Dracula"
                            "EdgeDark"
                            "EdgeLight"
                            "Eighties"
                            "EmbersLight"
                            "Embers"
                            "Emil"
                            "EquilibriumDark"
                            "EquilibriumGrayDark"
                            "EquilibriumGrayLight"
                            "EquilibriumLight"
                            "Eris"
                            "Espresso"
                            "EvaDim"
                            "Eva"
                            "EvenokDark"
                            "EverforestDarkHard"
                            "Everforest"
                            "Flat"
                            "Framer"
                            "FruitSoda"
                            "Gigavolt"
                            "Github"
                            "GoogleDark"
                            "GoogleLight"
                            "Gotham"
                            "GrayscaleDark"
                            "GrayscaleLight"
                            "Greenscreen"
                            "Gruber"
                            "GruvboxDarkHard"
                            "GruvboxDarkMedium"
                            "GruvboxDarkPale"
                            "GruvboxDarkSoft"
                            "GruvboxLightHard"
                            "GruvboxLightMedium"
                            "GruvboxLightSoft"
                            "GruvboxMaterialDarkHard"
                            "GruvboxMaterialDarkMedium"
                            "GruvboxMaterialDarkSoft"
                            "GruvboxMaterialLightHard"
                            "GruvboxMaterialLightMedium"
                            "GruvboxMaterialLightSoft"
                            "Hardcore"
                            "Harmonic16Dark"
                            "Harmonic16Light"
                            "HeetchLight"
                            "Heetch"
                            "Helios"
                            "Hopscotch"
                            "HorizonDark"
                            "HorizonLight"
                            "HorizonTerminalDark"
                            "HorizonTerminalLight"
                            "HumanoidDark"
                            "HumanoidLight"
                            "IaDark"
                            "IaLight"
                            "Icy"
                            "Irblack"
                            "Isotope"
                            "Jabuti"
                            "Kanagawa"
                            "Katy"
                            "Kimber"
                            "Lime"
                            "Macintosh"
                            "Marrakesh"
                            "Materia"
                            "MaterialDarker"
                            "MaterialLighter"
                            "MaterialPalenight"
                            "MaterialVivid"
                            "Material"
                            "MeasuredDark"
                            "MeasuredLight"
                            "MellowPurple"
                            "MexicoLight"
                            "Mocha"
                            "Monokai"
                            "Moonlight"
                            "Mountain"
                            "Nebula"
                            "NordLight"
                            "Nord"
                            "Nova"
                            "Ocean"
                            "Oceanicnext"
                            "OneLight"
                            "OnedarkDark"
                            "Onedark"
                            "OutrunDark"
                            "OxocarbonDark"
                            "OxocarbonLight"
                            "Pandora"
                            "PapercolorDark"
                            "PapercolorLight"
                            "Paraiso"
                            "Pasque"
                            "Phd"
                            "Pico"
                            "Pinky"
                            "Pop"
                            "Porple"
                            "PreciousDarkEleven"
                            "PreciousDarkFifteen"
                            "PreciousLightWarm"
                            "PreciousLightWhite"
                            "PrimerDarkDimmed"
                            "PrimerDark"
                            "PrimerLight"
                            "Purpledream"
                            "Qualia"
                            "Railscasts"
                            "Rebecca"
                            "RosePineDawn"
                            "RosePineMoon"
                            "RosePine"
                            "Saga"
                            "Sagelight"
                            "Sakura"
                            "Sandcastle"
                            "SelenizedBlack"
                            "SelenizedDark"
                            "SelenizedLight"
                            "SelenizedWhite"
                            "Seti"
                            "ShadesOfPurple"
                            "ShadesmearDark"
                            "ShadesmearLight"
                            "Shapeshifter"
                            "SilkDark"
                            "SilkLight"
                            "Snazzy"
                            "SolarflareLight"
                            "Solarflare"
                            "SolarizedDark"
                            "SolarizedLight"
                            "Spaceduck"
                            "Spacemacs"
                            "Sparky"
                            "StandardizedDark"
                            "StandardizedLight"
                            "Stella"
                            "StillAlive"
                            "Summercamp"
                            "SummerfruitDark"
                            "SummerfruitLight"
                            "SynthMidnightDark"
                            "SynthMidnightLight"
                            "Tango"
                            "Tarot"
                            "Tender"
                            "TerracottaDark"
                            "Terracotta"
                            "TokyoCityDark"
                            "TokyoCityLight"
                            "TokyoCityTerminalDark"
                            "TokyoCityTerminalLight"
                            "TokyoNightDark"
                            "TokyoNightLight"
                            "TokyoNightMoon"
                            "TokyoNightStorm"
                            "TokyoNightTerminalDark"
                            "TokyoNightTerminalLight"
                            "TokyoNightTerminalStorm"
                            "TokyodarkTerminal"
                            "Tokyodark"
                            "TomorrowNightEighties"
                            "TomorrowNight"
                            "Tomorrow"
                            "Tube"
                            "Twilight"
                            "UnikittyDark"
                            "UnikittyLight"
                            "UnikittyReversible"
                            "Uwunicorn"
                            "Vesper"
                            "Vice"
                            "Vulcan"
                            "Windows10Light"
                            "Windows10"
                            "Windows95Light"
                            "Windows95"
                            "WindowsHighcontrastLight"
                            "WindowsHighcontrast"
                            "WindowsNtLight"
                            "WindowsNt"
                            "Woodland"
                            "XcodeDusk"
                            "Zenbones"
                            "Zenburn"
                          ])
                        ]
                      )
                    );
                    default = null;
                  };
                  colours = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.submodule {
                          options = {
                            base_00 = lib.mkOption {
                              type = colour;
                              description = "Base00";
                            };
                            base_01 = lib.mkOption {
                              type = colour;
                              description = "Base01";
                            };
                            base_02 = lib.mkOption {
                              type = colour;
                              description = "Base02";
                            };
                            base_03 = lib.mkOption {
                              type = colour;
                              description = "Base03";
                            };
                            base_04 = lib.mkOption {
                              type = colour;
                              description = "Base04";
                            };
                            base_05 = lib.mkOption {
                              type = colour;
                              description = "Base05";
                            };
                            base_06 = lib.mkOption {
                              type = colour;
                              description = "Base06";
                            };
                            base_07 = lib.mkOption {
                              type = colour;
                              description = "Base07";
                            };
                            base_08 = lib.mkOption {
                              type = colour;
                              description = "Base08";
                            };
                            base_09 = lib.mkOption {
                              type = colour;
                              description = "Base09";
                            };
                            base_0a = lib.mkOption {
                              type = colour;
                              description = "Base0A";
                            };
                            base_0b = lib.mkOption {
                              type = colour;
                              description = "Base0B";
                            };
                            base_0c = lib.mkOption {
                              type = colour;
                              description = "Base0C";
                            };
                            base_0d = lib.mkOption {
                              type = colour;
                              description = "Base0D";
                            };
                            base_0e = lib.mkOption {
                              type = colour;
                              description = "Base0E";
                            };
                            base_0f = lib.mkOption {
                              type = colour;
                              description = "Base0F";
                            };
                          };
                        }
                      )
                    );
                    default = null;
                  };
                  auto_select_fill = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.oneOf [
                          (lib.types.nullOr catppuccinValue)
                          (lib.types.nullOr base16Value)
                        ]
                      )
                    );
                    default = null;
                  };
                  accent = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.oneOf [
                          (lib.types.nullOr catppuccinValue)
                          (lib.types.nullOr base16Value)
                        ]
                      )
                    );
                    default = null;
                  };
                };
              }
            )
          );
          default = null;
          description = "Theme";
        };
        transparency_alpha = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 200;
          description = "Alpha value for the color transparency [[0-255]]";
        };
        widget_spacing = lib.mkOption {
          type = (lib.types.nullOr lib.types.number);
          default = 10;
          description = "Spacing between widgets";
        };
      };
    };
    default = { };
    description = "komorebi for Mac bar configuration";
  };
}
