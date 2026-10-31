{ lib, ... }:
let
  animationStyle = (
    lib.types.oneOf [
      (lib.types.enum [
        "Linear"
        "EaseInSine"
        "EaseOutSine"
        "EaseInOutSine"
        "EaseInQuad"
        "EaseOutQuad"
        "EaseInOutQuad"
        "EaseInCubic"
        "EaseOutCubic"
        "EaseInOutCubic"
        "EaseInQuart"
        "EaseOutQuart"
        "EaseInOutQuart"
        "EaseInQuint"
        "EaseOutQuint"
        "EaseInOutQuint"
        "EaseInExpo"
        "EaseOutExpo"
        "EaseInOutExpo"
        "EaseInCirc"
        "EaseOutCirc"
        "EaseInOutCirc"
        "EaseInBack"
        "EaseOutBack"
        "EaseInOutBack"
        "EaseInElastic"
        "EaseOutElastic"
        "EaseInOutElastic"
        "EaseInBounce"
        "EaseOutBounce"
        "EaseInOutBounce"
      ])
      (lib.types.submodule {
        options = {
          CubicBezier = lib.mkOption {
            type = (lib.types.nullOr (lib.types.listOf lib.types.number));
            default = null;
          };
        };
      })
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
  defaultLayout = (
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
  );
  floatingLayerBehaviour = (
    lib.types.enum [
      "Tile"
      "Float"
    ]
  );
  idWithIdentifier = (
    lib.types.submodule {
      options = {
        id = lib.mkOption {
          type = lib.types.str;
          description = "Target identifier";
        };
        kind = lib.mkOption {
          type = (
            lib.types.enum [
              "Exe"
              "Class"
              "Title"
              "Path"
            ]
          );
          description = "Kind of identifier to target";
        };
        matching_strategy = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.enum [
                "Legacy"
                "Equals"
                "StartsWith"
                "EndsWith"
                "Contains"
                "Regex"
                "DoesNotEndWith"
                "DoesNotStartWith"
                "DoesNotEqual"
                "DoesNotContain"
              ]
            )
          );
          default = null;
          description = "Matching strategy to use";
        };
      };
    }
  );
  layoutOptions = (
    lib.types.submodule {
      options = {
        column_ratios = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf (lib.types.nullOr lib.types.number)));
          default = null;
          description = "Column width ratios (up to MAX_RATIOS values between 0.1 and 0.9)\n\n- Used by Columns layout: ratios for each column width\n- Used by Grid layout: ratios for column widths\n- Used by BSP, VerticalStack, RightMainVerticalStack: column_ratios[0] as primary split ratio\n- Used by HorizontalStack: column_ratios[0] as primary split ratio (top area height)\n- Used by UltrawideVerticalStack: column_ratios[0] as center ratio, column_ratios[1] as left ratio\n\nColumns without a ratio share remaining space equally.\nExample: `[0.3, 0.4, 0.3]` for 30%-40%-30% columns";
        };
        grid = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  rows = lib.mkOption {
                    type = lib.types.int;
                    description = "Maximum number of rows per grid column";
                  };
                };
              }
            )
          );
          default = null;
          description = "Options related to the Grid layout";
        };
        row_ratios = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf (lib.types.nullOr lib.types.number)));
          default = null;
          description = "Row height ratios (up to MAX_RATIOS values between 0.1 and 0.9)\n\n- Used by Rows layout: ratios for each row height\n- Used by Grid layout: ratios for row heights\n\nRows without a ratio share remaining space equally.\nExample: `[0.5, 0.5]` for 50%-50% rows";
        };
        scrolling = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  center_focused_column = lib.mkOption {
                    type = lib.types.anything;
                    default = null;
                    description = "When to centre the focused column: `Never` (the default), `Always`, or `OnOverflow`, which\nis when it does not fit on the screen together with the column that focus came from. A\nboolean is also accepted, where `true` is `Always` and `false` is `Never`";
                  };
                  columns = lib.mkOption {
                    type = lib.types.int;
                    description = "Desired number of visible columns (default: 3)";
                  };
                };
              }
            )
          );
          default = null;
          description = "Options related to the Scrolling layout";
        };
      };
    }
  );
  matchingRule = (
    lib.types.oneOf [
      idWithIdentifier
      (lib.types.listOf idWithIdentifier)
    ]
  );
  pathBuf = lib.types.str;
  placement = (
    lib.types.enum [
      "None"
      "Center"
      "CenterAndResize"
    ]
  );
  rect = (
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
  );
  wallpaper = (
    lib.types.submodule {
      options = {
        generate_theme = lib.mkOption {
          type = (lib.types.nullOr lib.types.bool);
          default = true;
          description = "Generate and apply Base16 theme for this wallpaper";
        };
        path = lib.mkOption {
          type = pathBuf;
          description = "Path to the wallpaper image file";
        };
        theme_options = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  bar_accent = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base0D";
                    description = "Komorebi status bar accent";
                  };
                  floating_border = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base09";
                    description = "Border colour when the window is floating";
                  };
                  monocle_border = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base0F";
                    description = "Border colour when the container is in monocle mode";
                  };
                  single_border = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base0D";
                    description = "Border colour when the container contains a single window";
                  };
                  stack_border = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base0B";
                    description = "Border colour when the container contains multiple windows";
                  };
                  theme_variant = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.enum [
                          "Dark"
                          "Light"
                        ]
                      )
                    );
                    default = "Dark";
                    description = "Specify Light or Dark variant for theme generation";
                  };
                  unfocused_border = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base01";
                    description = "Border colour when the container is unfocused";
                  };
                  unfocused_locked_border = lib.mkOption {
                    type = (lib.types.nullOr base16Value);
                    default = "Base08";
                    description = "Border colour when the container is unfocused and locked";
                  };
                };
              }
            )
          );
          default = null;
          description = "Specify theme options";
        };
      };
    }
  );
  windowContainerBehaviour = (
    lib.types.enum [
      "Create"
      "Append"
    ]
  );
in
{
  options.komorebi = lib.mkOption {
    type = lib.types.submodule {
      options = {
        animation = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  duration = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.oneOf [
                          (lib.types.submodule {
                            options = {
                              movement = lib.mkOption {
                                type = lib.types.int;
                              };
                              workspace = lib.mkOption {
                                type = lib.types.int;
                              };
                            };
                          })
                          lib.types.int
                        ]
                      )
                    );
                    default = 250;
                    description = "Set the animation duration in ms";
                  };
                  enabled = lib.mkOption {
                    type = (
                      lib.types.oneOf [
                        (lib.types.submodule {
                          options = {
                            movement = lib.mkOption {
                              type = lib.types.bool;
                            };
                            workspace = lib.mkOption {
                              type = lib.types.bool;
                            };
                          };
                        })
                        lib.types.bool
                      ]
                    );
                    default = false;
                    description = "Enable or disable animations";
                  };
                  fps = lib.mkOption {
                    type = (lib.types.nullOr lib.types.int);
                    default = 60;
                    description = "Set the animation FPS";
                  };
                  style = lib.mkOption {
                    type = (
                      lib.types.nullOr (
                        lib.types.oneOf [
                          (lib.types.submodule {
                            options = {
                              movement = lib.mkOption {
                                type = animationStyle;
                              };
                              workspace = lib.mkOption {
                                type = animationStyle;
                              };
                            };
                          })
                          animationStyle
                        ]
                      )
                    );
                    default = "Linear";
                    description = "Set the animation style";
                  };
                };
              }
            )
          );
          default = null;
          description = "Animations configuration options";
        };
        app_specific_configuration_path = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.oneOf [
                pathBuf
                (lib.types.listOf pathBuf)
              ]
            )
          );
          default = null;
          description = "Path to applications.json from komorebi-application-specific-configurations";
        };
        border = lib.mkOption {
          type = (lib.types.nullOr lib.types.bool);
          default = true;
          description = "Display window borders";
        };
        border_colours = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.submodule {
                options = {
                  floating = lib.mkOption {
                    type = (lib.types.nullOr colour);
                    default = null;
                    description = "Border colour when the container is in floating mode";
                  };
                  monocle = lib.mkOption {
                    type = (lib.types.nullOr colour);
                    default = null;
                    description = "Border colour when the container is in monocle mode";
                  };
                  single = lib.mkOption {
                    type = (lib.types.nullOr colour);
                    default = null;
                    description = "Border colour when the container contains a single window";
                  };
                  stack = lib.mkOption {
                    type = (lib.types.nullOr colour);
                    default = null;
                    description = "Border colour when the container contains multiple windows";
                  };
                  unfocused = lib.mkOption {
                    type = (lib.types.nullOr colour);
                    default = null;
                    description = "Border colour when the container is unfocused";
                  };
                  unfocused_locked = lib.mkOption {
                    type = (lib.types.nullOr colour);
                    default = null;
                    description = "Border colour when the container is unfocused and locked";
                  };
                };
              }
            )
          );
          default = null;
          description = "Window border colours for different container types (has no effect if a theme is defined)";
        };
        border_offset = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 0;
          description = "Offset of window borders";
        };
        border_radius = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 10;
          description = "Radius of window borders";
        };
        border_width = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 10;
          description = "Width of window borders";
        };
        cross_boundary_behaviour = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.enum [
                "Workspace"
                "Monitor"
              ]
            )
          );
          default = "Monitor";
          description = "Determine what happens when an action is called on a window at a monitor boundary";
        };
        cross_monitor_move_behaviour = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.enum [
                "Swap"
                "Insert"
                "NoOp"
              ]
            )
          );
          default = "Swap";
          description = "Determine what happens when a window is moved across a monitor boundary";
        };
        default_container_padding = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 5;
          description = "Global default container padding";
        };
        default_workspace_padding = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 5;
          description = "Global default workspace padding";
        };
        display_index_preferences = lib.mkOption {
          type = (lib.types.nullOr (lib.types.attrsOf lib.types.str));
          default = null;
          description = "Set display index preferences";
        };
        float_override = lib.mkOption {
          type = (lib.types.nullOr lib.types.bool);
          default = false;
          description = "Enable or disable float override, which makes it so every new window opens in floating mode";
        };
        float_override_placement = lib.mkOption {
          type = (lib.types.nullOr placement);
          default = null;
          description = "Determines the `Placement` to be used when spawning a window with float override active";
        };
        float_rule_placement = lib.mkOption {
          type = (lib.types.nullOr placement);
          default = null;
          description = "Determines the `Placement` to be used when spawning a window that matches a\n'floating_applications' rule";
        };
        floating_applications = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf matchingRule));
          default = null;
          description = "Identify applications which should be managed as floating windows";
        };
        floating_layer_behaviour = lib.mkOption {
          type = (lib.types.nullOr floatingLayerBehaviour);
          default = "Tile";
          description = "Determines what happens on a new window when on the `FloatingLayer`";
        };
        floating_layer_placement = lib.mkOption {
          type = (lib.types.nullOr placement);
          default = "Center";
          description = "Determines the `Placement` to be used when spawning a window on the floating layer with the\n`FloatingLayerBehaviour` set to `FloatingLayerBehaviour::Float`";
        };
        floating_window_aspect_ratio = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.oneOf [
                (lib.types.enum [
                  "Ultrawide"
                  "Widescreen"
                  "Standard"
                ])
                (lib.types.listOf lib.types.int)
              ]
            )
          );
          default = null;
          description = "Identify applications which are slow to send initial event notifications\nAspect ratio to resize with when toggling floating mode for a window";
        };
        global_work_area_offset = lib.mkOption {
          type = (lib.types.nullOr rect);
          default = null;
          description = "Global work area (space used for tiling) offset";
        };
        ignore_rules = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf matchingRule));
          default = null;
          description = "Individual window floating rules";
        };
        layout_defaults = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.attrsOf (
                lib.types.submodule {
                  options = {
                    layout_options = lib.mkOption {
                      type = (lib.types.nullOr layoutOptions);
                      default = null;
                      description = "Default layout options for this layout";
                    };
                    layout_options_rules = lib.mkOption {
                      type = (lib.types.nullOr (lib.types.attrsOf layoutOptions));
                      default = null;
                      description = "Threshold-based layout options rules in the format of threshold => options.\nWhen container count >= threshold, the highest matching threshold's options\nfully replace the base `layout_options`.";
                    };
                  };
                }
              )
            )
          );
          default = null;
          description = "Per-layout default options and rules, keyed by layout name.\nApplied as fallback when a workspace does not define its own layout_options or layout_options_rules.\nIf a workspace defines either setting, all global defaults for that layout are completely replaced.";
        };
        manage_rules = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf matchingRule));
          default = null;
          description = "Individual window force-manage rules";
        };
        monitors = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.listOf (
                lib.types.submodule {
                  options = {
                    container_padding = lib.mkOption {
                      type = (lib.types.nullOr lib.types.int);
                      default = null;
                      description = "Container padding";
                    };
                    floating_layer_behaviour = lib.mkOption {
                      type = (lib.types.nullOr floatingLayerBehaviour);
                      default = "Tile";
                      description = "Determine what happens to a new window when the Floating workspace layer is active";
                    };
                    wallpaper = lib.mkOption {
                      type = (lib.types.nullOr wallpaper);
                      default = null;
                      description = "Specify a wallpaper for this monitor";
                    };
                    window_based_work_area_offset = lib.mkOption {
                      type = (lib.types.nullOr rect);
                      default = null;
                      description = "Window based work area offset";
                    };
                    window_based_work_area_offset_limit = lib.mkOption {
                      type = (lib.types.nullOr lib.types.int);
                      default = 1;
                      description = "Open window limit after which the window based work area offset will no longer be applied";
                    };
                    window_hiding_position = lib.mkOption {
                      type = (
                        lib.types.nullOr (
                          lib.types.enum [
                            "BottomLeft"
                            "BottomRight"
                          ]
                        )
                      );
                      default = null;
                      description = "Determine which position windows should be hidden at on this monitor";
                    };
                    work_area_offset = lib.mkOption {
                      type = (lib.types.nullOr rect);
                      default = null;
                      description = "Monitor-specific work area offset";
                    };
                    workspace_padding = lib.mkOption {
                      type = (lib.types.nullOr lib.types.int);
                      default = null;
                      description = "Workspace padding (default: global)";
                    };
                    workspaces = lib.mkOption {
                      type = (
                        lib.types.listOf (
                          lib.types.submodule {
                            options = {
                              apply_window_based_work_area_offset = lib.mkOption {
                                type = (lib.types.nullOr lib.types.bool);
                                default = true;
                                description = "Apply this monitor's window-based work area offset";
                              };
                              container_padding = lib.mkOption {
                                type = (lib.types.nullOr lib.types.int);
                                default = null;
                                description = "Container padding (default: global)";
                              };
                              float_override = lib.mkOption {
                                type = (lib.types.nullOr lib.types.bool);
                                default = false;
                                description = "Enable or disable float override, which makes it so every new window opens in floating mode";
                              };
                              floating_layer_behaviour = lib.mkOption {
                                type = (lib.types.nullOr floatingLayerBehaviour);
                                default = "Tile";
                                description = "Determine what happens to a new window when the Floating workspace layer is active";
                              };
                              initial_workspace_rules = lib.mkOption {
                                type = (lib.types.nullOr (lib.types.listOf matchingRule));
                                default = null;
                                description = "Initial workspace application rules";
                              };
                              layout = lib.mkOption {
                                type = (lib.types.nullOr defaultLayout);
                                default = "BSP";
                                description = "Layout";
                              };
                              layout_flip = lib.mkOption {
                                type = (
                                  lib.types.nullOr (
                                    lib.types.enum [
                                      "Horizontal"
                                      "Vertical"
                                      "HorizontalAndVertical"
                                    ]
                                  )
                                );
                                default = null;
                                description = "Specify an axis on which to flip the selected layout";
                              };
                              layout_options = lib.mkOption {
                                type = (lib.types.nullOr layoutOptions);
                                default = null;
                                description = "Layout-specific options";
                              };
                              layout_options_rules = lib.mkOption {
                                type = (lib.types.nullOr (lib.types.attrsOf layoutOptions));
                                default = null;
                                description = "Threshold-based layout options rules in the format of threshold => options.\nWhen container count >= threshold, the highest matching threshold's options\nfully replace the base `layout_options`.\nThis follows the same threshold logic as `layout_rules`.";
                              };
                              layout_rules = lib.mkOption {
                                type = (lib.types.nullOr (lib.types.attrsOf defaultLayout));
                                default = null;
                                description = "Layout rules in the format of threshold => layout";
                              };
                              name = lib.mkOption {
                                type = lib.types.str;
                                description = "Name";
                              };
                              tile = lib.mkOption {
                                type = (lib.types.nullOr lib.types.bool);
                                default = true;
                                description = "Enable or disable tiling for the workspace";
                              };
                              wallpaper = lib.mkOption {
                                type = (lib.types.nullOr wallpaper);
                                default = null;
                                description = "Specify a wallpaper for this workspace";
                              };
                              window_container_behaviour = lib.mkOption {
                                type = (lib.types.nullOr windowContainerBehaviour);
                                default = "Create";
                                description = "Determine what happens when a new window is opened";
                              };
                              window_container_behaviour_rules = lib.mkOption {
                                type = (lib.types.nullOr (lib.types.attrsOf windowContainerBehaviour));
                                default = null;
                                description = "Window container behaviour rules in the format of threshold => behaviour";
                              };
                              work_area_offset = lib.mkOption {
                                type = (lib.types.nullOr rect);
                                default = null;
                                description = "Workspace specific work area offset";
                              };
                              work_area_offset_rules = lib.mkOption {
                                type = (lib.types.nullOr (lib.types.attrsOf rect));
                                default = null;
                                description = "Work area offset rules in the format of threshold => Rect (default: None)";
                              };
                              workspace_padding = lib.mkOption {
                                type = (lib.types.nullOr lib.types.int);
                                default = null;
                                description = "Workspace padding (default: global)";
                              };
                              workspace_rules = lib.mkOption {
                                type = (lib.types.nullOr (lib.types.listOf matchingRule));
                                default = null;
                                description = "Permanent workspace application rules";
                              };
                            };
                          }
                        )
                      );
                      description = "Workspace configurations";
                    };
                  };
                }
              )
            )
          );
          default = null;
          description = "Monitor and workspace configurations";
        };
        monocle_focus_behaviour = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.enum [
                "Cycle"
                "NoOp"
              ]
            )
          );
          default = "NoOp";
          description = "Determine what happens when focusing in a direction while a monocle container is active";
        };
        mouse_follows_focus = lib.mkOption {
          type = (lib.types.nullOr lib.types.bool);
          default = true;
          description = "Enable or disable mouse follows focus";
        };
        resize_delta = lib.mkOption {
          type = (lib.types.nullOr lib.types.int);
          default = 50;
          description = "Delta to resize windows by";
        };
        tabbed_applications = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf lib.types.str));
          default = null;
          description = "Identify applications which use native tabs for special handling";
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
                  bar_accent = lib.mkOption {
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
                  unfocused_border = lib.mkOption {
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
                  monocle_border = lib.mkOption {
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
                  stack_border = lib.mkOption {
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
                  floating_border = lib.mkOption {
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
                  unfocused_locked_border = lib.mkOption {
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
                  single_border = lib.mkOption {
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
                };
              }
            )
          );
          default = null;
          description = "Theme configuration options\n\nIf a theme is specified, `border_colours` will have no effect";
        };
        titleless_applications = lib.mkOption {
          type = (lib.types.nullOr (lib.types.listOf lib.types.str));
          default = null;
          description = "Identify applications which should be managed despite not reporting titles to the system";
        };
        toggle_float_placement = lib.mkOption {
          type = (lib.types.nullOr placement);
          default = "CenterAndResize";
          description = "Determines the placement of a new window when toggling to float";
        };
        unmanaged_window_operation_behaviour = lib.mkOption {
          type = (
            lib.types.nullOr (
              lib.types.enum [
                "Op"
                "NoOp"
              ]
            )
          );
          default = "Op";
          description = "Determine what happens when commands are sent while an unmanaged window is in the foreground";
        };
        window_container_behaviour = lib.mkOption {
          type = (lib.types.nullOr windowContainerBehaviour);
          default = "Create";
          description = "Determine what happens when a new window is opened";
        };
      };
    };
    default = { };
    description = "komorebi for Mac configuration";
  };
}
