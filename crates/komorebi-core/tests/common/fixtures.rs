//! Real `komorebic state` output of the three daemons, copied from `e2e/lib/tests/fixtures`
//! (Windows `kfw-*`, KDE `kfde-*`) as Rust text, because the nix source filters keep `.rs` files and
//! not `.json`. `state_roundtrip.rs` checks the copies against the originals when those exist.

#![allow(dead_code)]

pub const KFW_TILED: &str = r####"{
  "monitors": {
    "elements": [
      {
        "id": 65537,
        "name": "DISPLAY1",
        "device": "Default_Monitor",
        "device_id": "Default_Monitor-4&183790ca&0&UID0",
        "serial_number_id": null,
        "size": {
          "left": 0,
          "top": 0,
          "right": 1280,
          "bottom": 800
        },
        "work_area_size": {
          "left": 0,
          "top": 0,
          "right": 1280,
          "bottom": 752
        },
        "work_area_offset": null,
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": {
          "elements": [
            {
              "name": null,
              "containers": {
                "elements": [
                  {
                    "id": "lHDkMLvTxsJGQfBTiKnkt",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 262540,
                          "title": "win1",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 27,
                            "top": 27,
                            "right": 596,
                            "bottom": 698
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "Zqsuvee3D_fegP33OgBnq",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 131234,
                          "title": "win2",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 657,
                            "top": 27,
                            "right": 596,
                            "bottom": 332
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "fop08rEXwYLbkqzpy3nE9",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 327778,
                          "title": "win3",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 657,
                            "top": 393,
                            "right": 596,
                            "bottom": 332
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 2
              },
              "monocle_container": null,
              "maximized_window": null,
              "floating_windows": {
                "elements": [],
                "focused": 0
              },
              "layout": {
                "Default": "BSP"
              },
              "layout_options": null,
              "layout_rules": [],
              "layout_options_rules": [],
              "work_area_offset_rules": [],
              "layout_flip": null,
              "workspace_padding": 10,
              "container_padding": 10,
              "latest_layout": [
                {
                  "left": 27,
                  "top": 27,
                  "right": 596,
                  "bottom": 698
                },
                {
                  "left": 657,
                  "top": 27,
                  "right": 596,
                  "bottom": 332
                },
                {
                  "left": 657,
                  "top": 393,
                  "right": 596,
                  "bottom": 332
                }
              ],
              "resize_dimensions": [
                null,
                null,
                null
              ],
              "tile": true,
              "work_area_offset": null,
              "apply_window_based_work_area_offset": true,
              "window_container_behaviour": null,
              "window_container_behaviour_rules": null,
              "float_override": null,
              "layer": "Tiling",
              "floating_layer_behaviour": null,
              "wallpaper": null
            }
          ],
          "focused": 0
        },
        "workspace_names": {},
        "container_padding": null,
        "workspace_padding": null,
        "wallpaper": null,
        "floating_layer_behaviour": null
      }
    ],
    "focused": 0
  },
  "monitor_usr_idx_map": {
    "0": 0
  },
  "is_paused": false,
  "resize_delta": 50,
  "new_window_behaviour": "Create",
  "float_override": false,
  "cross_monitor_move_behaviour": "Swap",
  "monocle_focus_behaviour": "NoOp",
  "unmanaged_window_operation_behaviour": "Op",
  "work_area_offset": null,
  "focus_follows_mouse": null,
  "mouse_follows_focus": true,
  "has_pending_raise_op": false,
  "virtual_desktop_id": null
}"####;

pub const KFW_STACK: &str = r####"{
  "monitors": {
    "elements": [
      {
        "id": 65537,
        "name": "DISPLAY1",
        "device": "Default_Monitor",
        "device_id": "Default_Monitor-4&183790ca&0&UID0",
        "serial_number_id": null,
        "size": {
          "left": 0,
          "top": 0,
          "right": 1280,
          "bottom": 800
        },
        "work_area_size": {
          "left": 0,
          "top": 0,
          "right": 1280,
          "bottom": 752
        },
        "work_area_offset": null,
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": {
          "elements": [
            {
              "name": null,
              "containers": {
                "elements": [
                  {
                    "id": "lHDkMLvTxsJGQfBTiKnkt",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 262540,
                          "title": "win1",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 27,
                            "top": 27,
                            "right": 596,
                            "bottom": 698
                          }
                        },
                        {
                          "hwnd": 327778,
                          "title": "win3",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 27,
                            "top": 27,
                            "right": 596,
                            "bottom": 698
                          }
                        }
                      ],
                      "focused": 1
                    }
                  },
                  {
                    "id": "Zqsuvee3D_fegP33OgBnq",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 131234,
                          "title": "win2",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 657,
                            "top": 27,
                            "right": 596,
                            "bottom": 698
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 0
              },
              "monocle_container": null,
              "maximized_window": null,
              "floating_windows": {
                "elements": [],
                "focused": 0
              },
              "layout": {
                "Default": "BSP"
              },
              "layout_options": null,
              "layout_rules": [],
              "layout_options_rules": [],
              "work_area_offset_rules": [],
              "layout_flip": null,
              "workspace_padding": 10,
              "container_padding": 10,
              "latest_layout": [
                {
                  "left": 27,
                  "top": 27,
                  "right": 596,
                  "bottom": 698
                },
                {
                  "left": 657,
                  "top": 27,
                  "right": 596,
                  "bottom": 698
                }
              ],
              "resize_dimensions": [
                null,
                null
              ],
              "tile": true,
              "work_area_offset": null,
              "apply_window_based_work_area_offset": true,
              "window_container_behaviour": null,
              "window_container_behaviour_rules": null,
              "float_override": null,
              "layer": "Tiling",
              "floating_layer_behaviour": null,
              "wallpaper": null
            }
          ],
          "focused": 0
        },
        "workspace_names": {},
        "container_padding": null,
        "workspace_padding": null,
        "wallpaper": null,
        "floating_layer_behaviour": null
      }
    ],
    "focused": 0
  },
  "monitor_usr_idx_map": {
    "0": 0
  },
  "is_paused": false,
  "resize_delta": 50,
  "new_window_behaviour": "Create",
  "float_override": false,
  "cross_monitor_move_behaviour": "Swap",
  "monocle_focus_behaviour": "NoOp",
  "unmanaged_window_operation_behaviour": "Op",
  "work_area_offset": null,
  "focus_follows_mouse": null,
  "mouse_follows_focus": true,
  "has_pending_raise_op": false,
  "virtual_desktop_id": null
}"####;

pub const KFW_FLOAT: &str = r####"{
  "monitors": {
    "elements": [
      {
        "id": 65537,
        "name": "DISPLAY1",
        "device": "Default_Monitor",
        "device_id": "Default_Monitor-4&183790ca&0&UID0",
        "serial_number_id": null,
        "size": {
          "left": 0,
          "top": 0,
          "right": 1280,
          "bottom": 800
        },
        "work_area_size": {
          "left": 0,
          "top": 0,
          "right": 1280,
          "bottom": 752
        },
        "work_area_offset": null,
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": {
          "elements": [
            {
              "name": null,
              "containers": {
                "elements": [
                  {
                    "id": "lHDkMLvTxsJGQfBTiKnkt",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 262540,
                          "title": "win1",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 27,
                            "top": 27,
                            "right": 596,
                            "bottom": 698
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "Zqsuvee3D_fegP33OgBnq",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 131234,
                          "title": "win2",
                          "exe": "komorebi-testwin.exe",
                          "class": "KomorebiTestWin",
                          "rect": {
                            "left": 657,
                            "top": 27,
                            "right": 596,
                            "bottom": 698
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 0
              },
              "monocle_container": null,
              "maximized_window": null,
              "floating_windows": {
                "elements": [
                  {
                    "hwnd": 327778,
                    "title": "win3",
                    "exe": "komorebi-testwin.exe",
                    "class": "KomorebiTestWin",
                    "rect": {
                      "left": 27,
                      "top": 27,
                      "right": 596,
                      "bottom": 698
                    }
                  }
                ],
                "focused": 0
              },
              "layout": {
                "Default": "BSP"
              },
              "layout_options": null,
              "layout_rules": [],
              "layout_options_rules": [],
              "work_area_offset_rules": [],
              "layout_flip": null,
              "workspace_padding": 10,
              "container_padding": 10,
              "latest_layout": [
                {
                  "left": 27,
                  "top": 27,
                  "right": 596,
                  "bottom": 698
                },
                {
                  "left": 657,
                  "top": 27,
                  "right": 596,
                  "bottom": 698
                }
              ],
              "resize_dimensions": [
                null,
                null
              ],
              "tile": true,
              "work_area_offset": null,
              "apply_window_based_work_area_offset": true,
              "window_container_behaviour": null,
              "window_container_behaviour_rules": null,
              "float_override": null,
              "layer": "Floating",
              "floating_layer_behaviour": null,
              "wallpaper": null
            }
          ],
          "focused": 0
        },
        "workspace_names": {},
        "container_padding": null,
        "workspace_padding": null,
        "wallpaper": null,
        "floating_layer_behaviour": null
      }
    ],
    "focused": 0
  },
  "monitor_usr_idx_map": {
    "0": 0
  },
  "is_paused": false,
  "resize_delta": 50,
  "new_window_behaviour": "Create",
  "float_override": false,
  "cross_monitor_move_behaviour": "Swap",
  "monocle_focus_behaviour": "NoOp",
  "unmanaged_window_operation_behaviour": "Op",
  "work_area_offset": null,
  "focus_follows_mouse": null,
  "mouse_follows_focus": true,
  "has_pending_raise_op": false,
  "virtual_desktop_id": null
}"####;

pub const KFDE_TILED: &str = r####"{
  "monitors": {
    "elements": [
      {
        "id": 1230296105,
        "device": "Virtual-0",
        "serial_number_id": "Virtual-0",
        "workspaces": {
          "elements": [
            {
              "name": null,
              "containers": {
                "elements": [
                  {
                    "id": "maAEzGA_0nRWS6PpcDLlI",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 4,
                          "rect": {
                            "left": 20,
                            "top": 20,
                            "right": 925,
                            "bottom": 1040
                          },
                          "details": {
                            "title": "neutral1",
                            "exe": "foot",
                            "class": "foot",
                            "path": "/nix/store/0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0/bin/foot"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "f63LOWKm57v9SDinvNb_K",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 6,
                          "rect": {
                            "left": 975,
                            "top": 20,
                            "right": 924,
                            "bottom": 504
                          },
                          "details": {
                            "title": "neutral3",
                            "exe": "foot",
                            "class": "foot",
                            "path": "/nix/store/0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0/bin/foot"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "z2cN_l7IEJBa53ymxMgvA",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 5,
                          "rect": {
                            "left": 975,
                            "top": 555,
                            "right": 924,
                            "bottom": 504
                          },
                          "details": {
                            "title": "neutral2",
                            "exe": "foot",
                            "class": "foot",
                            "path": "/nix/store/0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0/bin/foot"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 1
              },
              "monocle_container": null,
              "maximized_window": null,
              "floating_windows": {
                "elements": [],
                "focused": 0
              },
              "layout": {
                "Default": "BSP"
              },
              "layout_options": null,
              "layout_rules": [],
              "layout_options_rules": [],
              "layout_flip": null,
              "workspace_padding": null,
              "container_padding": null,
              "latest_layout": [
                {
                  "left": 20,
                  "top": 20,
                  "right": 925,
                  "bottom": 1040
                },
                {
                  "left": 975,
                  "top": 20,
                  "right": 925,
                  "bottom": 505
                },
                {
                  "left": 975,
                  "top": 555,
                  "right": 925,
                  "bottom": 505
                }
              ],
              "resize_dimensions": [
                null,
                null,
                null
              ],
              "tile": true,
              "work_area_offset": null,
              "work_area_offset_rules": [],
              "apply_window_based_work_area_offset": true,
              "window_container_behaviour": null,
              "window_container_behaviour_rules": null,
              "float_override": null,
              "layer": "Tiling",
              "floating_layer_behaviour": null,
              "wallpaper": null
            }
          ],
          "focused": 0
        },
        "size": {
          "left": 0,
          "top": 0,
          "right": 1920,
          "bottom": 1080
        },
        "work_area_offset": null,
        "work_area_size": {
          "left": 0,
          "top": 0,
          "right": 1920,
          "bottom": 1080
        },
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "container_padding": null,
        "workspace_padding": null,
        "last_focused_workspace": 0,
        "wallpaper": null,
        "floating_layer_behaviour": null
      }
    ],
    "focused": 0
  },
  "monitor_usr_idx_map": {
    "0": 0
  },
  "is_paused": false,
  "resize_delta": 50,
  "new_window_behaviour": "Create",
  "float_override": false,
  "cross_monitor_move_behaviour": "Swap",
  "monocle_focus_behaviour": "NoOp",
  "unmanaged_window_operation_behaviour": "Op",
  "work_area_offset": null,
  "focus_follows_mouse": null,
  "mouse_follows_focus": true
}"####;

pub const KFDE_STACK: &str = r####"{
  "monitors": {
    "elements": [
      {
        "id": 1230296105,
        "device": "Virtual-0",
        "serial_number_id": "Virtual-0",
        "workspaces": {
          "elements": [
            {
              "name": null,
              "containers": {
                "elements": [
                  {
                    "id": "maAEzGA_0nRWS6PpcDLlI",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 4,
                          "rect": {
                            "left": 20,
                            "top": 20,
                            "right": 925,
                            "bottom": 1040
                          },
                          "details": {
                            "title": "neutral1",
                            "exe": "foot",
                            "class": "foot",
                            "path": "/nix/store/0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0/bin/foot"
                          }
                        },
                        {
                          "id": 6,
                          "rect": {
                            "left": 20,
                            "top": 20,
                            "right": 924,
                            "bottom": 1037
                          },
                          "details": {
                            "title": "neutral3",
                            "exe": "foot",
                            "class": "foot",
                            "path": "/nix/store/0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0/bin/foot"
                          }
                        }
                      ],
                      "focused": 1
                    }
                  },
                  {
                    "id": "z2cN_l7IEJBa53ymxMgvA",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 5,
                          "rect": {
                            "left": 975,
                            "top": 20,
                            "right": 924,
                            "bottom": 1037
                          },
                          "details": {
                            "title": "neutral2",
                            "exe": "foot",
                            "class": "foot",
                            "path": "/nix/store/0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0/bin/foot"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 0
              },
              "monocle_container": null,
              "maximized_window": null,
              "floating_windows": {
                "elements": [],
                "focused": 0
              },
              "layout": {
                "Default": "BSP"
              },
              "layout_options": null,
              "layout_rules": [],
              "layout_options_rules": [],
              "layout_flip": null,
              "workspace_padding": null,
              "container_padding": null,
              "latest_layout": [
                {
                  "left": 20,
                  "top": 20,
                  "right": 925,
                  "bottom": 1040
                },
                {
                  "left": 975,
                  "top": 20,
                  "right": 925,
                  "bottom": 1040
                }
              ],
              "resize_dimensions": [
                null,
                null
              ],
              "tile": true,
              "work_area_offset": null,
              "work_area_offset_rules": [],
              "apply_window_based_work_area_offset": true,
              "window_container_behaviour": null,
              "window_container_behaviour_rules": null,
              "float_override": null,
              "layer": "Tiling",
              "floating_layer_behaviour": null,
              "wallpaper": null
            }
          ],
          "focused": 0
        },
        "size": {
          "left": 0,
          "top": 0,
          "right": 1920,
          "bottom": 1080
        },
        "work_area_offset": null,
        "work_area_size": {
          "left": 0,
          "top": 0,
          "right": 1920,
          "bottom": 1080
        },
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "container_padding": null,
        "workspace_padding": null,
        "last_focused_workspace": 0,
        "wallpaper": null,
        "floating_layer_behaviour": null
      }
    ],
    "focused": 0
  },
  "monitor_usr_idx_map": {
    "0": 0
  },
  "is_paused": false,
  "resize_delta": 50,
  "new_window_behaviour": "Create",
  "float_override": false,
  "cross_monitor_move_behaviour": "Swap",
  "monocle_focus_behaviour": "NoOp",
  "unmanaged_window_operation_behaviour": "Op",
  "work_area_offset": null,
  "focus_follows_mouse": null,
  "mouse_follows_focus": true
}"####;

/// Shaped like the macOS state (windows as `id`, `rect`, `details` with role and subrole; the monitor
/// with `window_hiding_position` last), derived from `kfde-tiled` with jq: NOT real macOS output, which
/// cannot be produced on this host. Replace it with the output of `komorebic state` on a Mac when
/// one is at hand.
pub const MACOS_TILED: &str = r####"{
  "monitors": {
    "elements": [
      {
        "id": 1,
        "device": "Virtual-0",
        "workspaces": {
          "elements": [
            {
              "name": null,
              "containers": {
                "elements": [
                  {
                    "id": "maAEzGA_0nRWS6PpcDLlI",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 4,
                          "rect": {
                            "left": 20,
                            "top": 20,
                            "right": 925,
                            "bottom": 1040
                          },
                          "details": {
                            "title": "neutral1",
                            "exe": "foot",
                            "role": "AXWindow",
                            "subrole": "AXStandardWindow",
                            "icon_path": "/Applications/Terminal.app/Contents/Resources/Terminal.icns"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "f63LOWKm57v9SDinvNb_K",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 6,
                          "rect": {
                            "left": 975,
                            "top": 20,
                            "right": 924,
                            "bottom": 504
                          },
                          "details": {
                            "title": "neutral3",
                            "exe": "foot",
                            "role": "AXWindow",
                            "subrole": "AXStandardWindow",
                            "icon_path": "/Applications/Terminal.app/Contents/Resources/Terminal.icns"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "id": "z2cN_l7IEJBa53ymxMgvA",
                    "locked": false,
                    "windows": {
                      "elements": [
                        {
                          "id": 5,
                          "rect": {
                            "left": 975,
                            "top": 555,
                            "right": 924,
                            "bottom": 504
                          },
                          "details": {
                            "title": "neutral2",
                            "exe": "foot",
                            "role": "AXWindow",
                            "subrole": "AXStandardWindow",
                            "icon_path": "/Applications/Terminal.app/Contents/Resources/Terminal.icns"
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 1
              },
              "monocle_container": null,
              "maximized_window": null,
              "floating_windows": {
                "elements": [],
                "focused": 0
              },
              "layout": {
                "Default": "BSP"
              },
              "layout_options": null,
              "layout_rules": [],
              "layout_options_rules": [],
              "layout_flip": null,
              "workspace_padding": null,
              "container_padding": null,
              "latest_layout": [
                {
                  "left": 20,
                  "top": 20,
                  "right": 925,
                  "bottom": 1040
                },
                {
                  "left": 975,
                  "top": 20,
                  "right": 925,
                  "bottom": 505
                },
                {
                  "left": 975,
                  "top": 555,
                  "right": 925,
                  "bottom": 505
                }
              ],
              "resize_dimensions": [
                null,
                null,
                null
              ],
              "tile": true,
              "work_area_offset": null,
              "work_area_offset_rules": [],
              "apply_window_based_work_area_offset": true,
              "window_container_behaviour": null,
              "window_container_behaviour_rules": null,
              "float_override": null,
              "layer": "Tiling",
              "floating_layer_behaviour": null,
              "wallpaper": null
            }
          ],
          "focused": 0
        },
        "size": {
          "left": 0,
          "top": 0,
          "right": 1920,
          "bottom": 1080
        },
        "work_area_offset": null,
        "work_area_size": {
          "left": 0,
          "top": 0,
          "right": 1920,
          "bottom": 1080
        },
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "container_padding": null,
        "workspace_padding": null,
        "last_focused_workspace": 0,
        "wallpaper": null,
        "floating_layer_behaviour": null,
        "window_hiding_position": "BottomLeft",
        "serial_number_id": "C02XXXXXXXX"
      }
    ],
    "focused": 0
  },
  "monitor_usr_idx_map": {
    "0": 0
  },
  "is_paused": false,
  "resize_delta": 50,
  "new_window_behaviour": "Create",
  "float_override": false,
  "cross_monitor_move_behaviour": "Swap",
  "monocle_focus_behaviour": "NoOp",
  "unmanaged_window_operation_behaviour": "Op",
  "work_area_offset": null,
  "focus_follows_mouse": null,
  "mouse_follows_focus": true
}"####;
