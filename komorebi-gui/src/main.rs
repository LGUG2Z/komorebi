#![warn(clippy::all)]

use eframe::egui;
use eframe::egui::Color32;
use eframe::egui::ViewportBuilder;
use eframe::egui::color_picker::Alpha;
use komorebi_client::BorderStyle;
use komorebi_client::Colour;
use komorebi_client::DefaultLayout;
use komorebi_client::GlobalState;
use komorebi_client::Layout;
use komorebi_client::Rect;
use komorebi_client::Rgb;
use komorebi_client::RuleDebug;
use komorebi_client::SocketMessage;
use komorebi_client::StackbarLabel;
use komorebi_client::StackbarMode;
use komorebi_client::State;
use komorebi_client::Window;
use komorebi_client::WindowKind;
use std::collections::HashMap;
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

fn main() {
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_always_on_top()
            .with_inner_size([320.0, 500.0]),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "komorebi-gui",
        native_options,
        Box::new(|cc| Ok(Box::new(KomorebiGui::new(cc)))),
    );
}

struct BorderColours {
    single: Color32,
    stack: Color32,
    monocle: Color32,
    floating: Color32,
    unfocused: Color32,
    unfocused_locked: Color32,
}

struct BorderConfig {
    border_enabled: bool,
    border_colours: BorderColours,
    border_style: BorderStyle,
    border_offset: i32,
    border_width: i32,
}

struct StackbarConfig {
    mode: StackbarMode,
    label: StackbarLabel,
    height: i32,
    width: i32,
    focused_text_colour: Color32,
    unfocused_text_colour: Color32,
    background_colour: Color32,
}

struct MonitorConfig {
    size: Rect,
    work_area_offset: Rect,
    workspaces: Vec<WorkspaceConfig>,
}

impl From<&komorebi_client::Monitor> for MonitorConfig {
    fn from(value: &komorebi_client::Monitor) -> Self {
        let mut workspaces = vec![];
        for ws in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(ws));
        }

        Self {
            size: value.size,
            work_area_offset: value.work_area_offset.unwrap_or_default(),
            workspaces,
        }
    }
}

struct WorkspaceConfig {
    name: String,
    tile: bool,
    layout: DefaultLayout,
    container_padding: i32,
    workspace_padding: i32,
}

impl From<&komorebi_client::Workspace> for WorkspaceConfig {
    fn from(value: &komorebi_client::Workspace) -> Self {
        let layout = match value.layout {
            Layout::Default(layout) => layout,
            Layout::Custom(_) => DefaultLayout::BSP,
        };

        let name = value
            .name
            .to_owned()
            .unwrap_or_else(|| random_word::get(random_word::Lang::En).to_string());

        Self {
            layout,
            name,
            tile: value.tile,
            workspace_padding: value.workspace_padding.unwrap_or(20),
            container_padding: value.container_padding.unwrap_or(20),
        }
    }
}

struct KomorebiGui {
    border_config: BorderConfig,
    stackbar_config: StackbarConfig,
    mouse_follows_focus: bool,
    monitors: Vec<MonitorConfig>,
    workspace_names: HashMap<usize, Vec<String>>,
    debug_hwnd: isize,
    debug_windows: Vec<Window>,
    debug_rule: Option<RuleDebug>,
}

fn colour32(colour: Option<Colour>) -> Color32 {
    match colour {
        Some(Colour::Rgb(rgb)) => Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8),
        Some(Colour::Hex(hex)) => {
            let rgb = Rgb::from(hex);
            Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8)
        }
        None => Color32::from_rgb(0, 0, 0),
    }
}

impl KomorebiGui {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        let global_state: GlobalState = serde_json::from_str(
            &komorebi_client::send_query(&SocketMessage::GlobalState).unwrap(),
        )
        .unwrap();

        let state: State =
            serde_json::from_str(&komorebi_client::send_query(&SocketMessage::State).unwrap())
                .unwrap();

        let border_colours = BorderColours {
            single: colour32(global_state.border_colours.single),
            stack: colour32(global_state.border_colours.stack),
            monocle: colour32(global_state.border_colours.monocle),
            floating: colour32(global_state.border_colours.floating),
            unfocused: colour32(global_state.border_colours.unfocused),
            unfocused_locked: colour32(global_state.border_colours.unfocused_locked),
        };

        let border_config = BorderConfig {
            border_enabled: global_state.border_enabled,
            border_colours,
            border_style: global_state.border_style,
            border_offset: global_state.border_offset,
            border_width: global_state.border_width,
        };

        let mut monitors = vec![];
        for m in state.monitors.elements() {
            monitors.push(MonitorConfig::from(m));
        }

        let mut workspace_names = HashMap::new();

        for (monitor_idx, m) in monitors.iter().enumerate() {
            for ws in &m.workspaces {
                let names = workspace_names.entry(monitor_idx).or_insert_with(Vec::new);
                names.push(ws.name.clone());
            }
        }

        let stackbar_config = StackbarConfig {
            mode: global_state.stackbar_mode,
            height: global_state.stackbar_height,
            width: global_state.stackbar_tab_width,
            label: global_state.stackbar_label,
            focused_text_colour: colour32(Some(global_state.stackbar_focused_text_colour)),
            unfocused_text_colour: colour32(Some(global_state.stackbar_unfocused_text_colour)),
            background_colour: colour32(Some(global_state.stackbar_tab_background_colour)),
        };

        let mut debug_windows = vec![];

        unsafe {
            EnumWindows(
                Some(enum_window),
                windows::Win32::Foundation::LPARAM(&mut debug_windows as *mut Vec<Window> as isize),
            )
            .unwrap();
        };

        Self {
            border_config,
            mouse_follows_focus: state.mouse_follows_focus,
            monitors,
            workspace_names,
            debug_hwnd: 0,
            debug_windows,
            stackbar_config,
            debug_rule: None,
        }
    }
}

extern "system" fn enum_window(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows_core::BOOL {
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<Window>) };
    let window = Window::from(hwnd.0 as isize);

    if window.is_window()
        && !window.is_miminized()
        && window.is_visible()
        && window.title().is_ok()
        && window.exe().is_ok()
    {
        windows.push(window);
    }

    true.into()
}

fn json_view_ui(ui: &mut egui::Ui, code: &str) {
    let language = "json";
    let theme =
        egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), &ui.ctx().style());
    egui_extras::syntax_highlighting::code_view_ui(ui, &theme, code, language);
}

impl eframe::App for KomorebiGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ctx.set_pixels_per_point(2.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(ctx.screen_rect().width());
                ui.collapsing("Debugging", |ui| {
                    ui.collapsing("Window Rules", |ui| {
                        let window = Window::from(self.debug_hwnd);

                        let label = if let (Ok(title), Ok(exe)) = (window.title(), window.exe()) {
                            format!("{title} ({exe})")
                        } else {
                            String::from("Select a Window")
                        };

                        if ui.button("Refresh Windows").clicked() {
                            let mut debug_windows = vec![];

                            unsafe {
                                EnumWindows(
                                    Some(enum_window),
                                    windows::Win32::Foundation::LPARAM(
                                        &mut debug_windows as *mut Vec<Window> as isize,
                                    ),
                                )
                                .unwrap();
                            };

                            self.debug_windows = debug_windows;
                        }

                        egui::ComboBox::from_label("Select a Window")
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for w in &self.debug_windows {
                                    if ui
                                        .selectable_value(
                                            &mut self.debug_hwnd,
                                            w.hwnd,
                                            format!(
                                                "{} ({})",
                                                w.title().unwrap(),
                                                w.exe().unwrap()
                                            ),
                                        )
                                        .changed()
                                    {
                                        let debug_rule: RuleDebug = serde_json::from_str(
                                            &komorebi_client::send_query(
                                                &SocketMessage::DebugWindow(self.debug_hwnd),
                                            )
                                            .unwrap(),
                                        )
                                        .unwrap();

                                        self.debug_rule = Some(debug_rule)
                                    }
                                }
                            });

                        if let Some(debug_rule) = &self.debug_rule {
                            json_view_ui(ui, &serde_json::to_string_pretty(debug_rule).unwrap())
                        }
                    });
                });

                ui.collapsing("Mouse", |ui| {
                    if ui
                        .toggle_value(&mut self.mouse_follows_focus, "Mouse Follows Focus")
                        .changed()
                    {
                        komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                            self.mouse_follows_focus,
                        ))
                        .unwrap();
                    }
                });

                ui.collapsing("Border", |ui| {
                    if ui
                        .toggle_value(&mut self.border_config.border_enabled, "Border")
                        .changed()
                    {
                        komorebi_client::send_message(&SocketMessage::Border(
                            self.border_config.border_enabled,
                        ))
                        .unwrap();
                    }

                    ui.collapsing("Colours", |ui| {
                        ui.collapsing("Single", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.single,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(&SocketMessage::BorderColour(
                                    WindowKind::Single,
                                    self.border_config.border_colours.single.r() as u32,
                                    self.border_config.border_colours.single.g() as u32,
                                    self.border_config.border_colours.single.b() as u32,
                                ))
                                .unwrap();
                            }
                        });

                        ui.collapsing("Stack", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.stack,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(&SocketMessage::BorderColour(
                                    WindowKind::Stack,
                                    self.border_config.border_colours.stack.r() as u32,
                                    self.border_config.border_colours.stack.g() as u32,
                                    self.border_config.border_colours.stack.b() as u32,
                                ))
                                .unwrap();
                            }
                        });

                        ui.collapsing("Monocle", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.monocle,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(&SocketMessage::BorderColour(
                                    WindowKind::Monocle,
                                    self.border_config.border_colours.monocle.r() as u32,
                                    self.border_config.border_colours.monocle.g() as u32,
                                    self.border_config.border_colours.monocle.b() as u32,
                                ))
                                .unwrap();
                            }
                        });

                        ui.collapsing("Floating", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.floating,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(&SocketMessage::BorderColour(
                                    WindowKind::Floating,
                                    self.border_config.border_colours.floating.r() as u32,
                                    self.border_config.border_colours.floating.g() as u32,
                                    self.border_config.border_colours.floating.b() as u32,
                                ))
                                .unwrap();
                            }
                        });

                        ui.collapsing("Unfocused", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.unfocused,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(&SocketMessage::BorderColour(
                                    WindowKind::Unfocused,
                                    self.border_config.border_colours.unfocused.r() as u32,
                                    self.border_config.border_colours.unfocused.g() as u32,
                                    self.border_config.border_colours.unfocused.b() as u32,
                                ))
                                .unwrap();
                            }
                        });

                        ui.collapsing("Unfocused Locked", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.unfocused_locked,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(&SocketMessage::BorderColour(
                                    WindowKind::UnfocusedLocked,
                                    self.border_config.border_colours.unfocused_locked.r() as u32,
                                    self.border_config.border_colours.unfocused_locked.g() as u32,
                                    self.border_config.border_colours.unfocused_locked.b() as u32,
                                ))
                                .unwrap();
                            }
                        })
                    });

                    ui.collapsing("Style", |ui| {
                        for option in [
                            BorderStyle::System,
                            BorderStyle::Rounded,
                            BorderStyle::Square,
                        ] {
                            if ui
                                .add(egui::SelectableLabel::new(
                                    self.border_config.border_style == option,
                                    option.to_string(),
                                ))
                                .clicked()
                            {
                                self.border_config.border_style = option;
                                komorebi_client::send_message(&SocketMessage::BorderStyle(
                                    self.border_config.border_style,
                                ))
                                .unwrap();

                                std::thread::sleep(Duration::from_secs(1));

                                komorebi_client::send_message(&SocketMessage::Retile).unwrap();
                            }
                        }
                    });

                    ui.collapsing("Width", |ui| {
                        if ui
                            .add(egui::Slider::new(
                                &mut self.border_config.border_width,
                                -50..=50,
                            ))
                            .changed()
                        {
                            komorebi_client::send_message(&SocketMessage::BorderWidth(
                                self.border_config.border_width,
                            ))
                            .unwrap();
                        };
                    });

                    ui.collapsing("Offset", |ui| {
                        if ui
                            .add(egui::Slider::new(
                                &mut self.border_config.border_offset,
                                -50..=50,
                            ))
                            .changed()
                        {
                            komorebi_client::send_message(&SocketMessage::BorderOffset(
                                self.border_config.border_offset,
                            ))
                            .unwrap();
                        };
                    });
                });

                ui.collapsing("Stackbar", |ui| {
                    for option in [
                        StackbarMode::Never,
                        StackbarMode::OnStack,
                        StackbarMode::Always,
                    ] {
                        if ui
                            .add(egui::SelectableLabel::new(
                                self.stackbar_config.mode == option,
                                option.to_string(),
                            ))
                            .clicked()
                        {
                            self.stackbar_config.mode = option;
                            komorebi_client::send_message(&SocketMessage::StackbarMode(
                                self.stackbar_config.mode,
                            ))
                            .unwrap();

                            komorebi_client::send_message(&SocketMessage::Retile).unwrap()
                        }
                    }

                    ui.collapsing("Label", |ui| {
                        for option in [StackbarLabel::Process, StackbarLabel::Title] {
                            if ui
                                .add(egui::SelectableLabel::new(
                                    self.stackbar_config.label == option,
                                    option.to_string(),
                                ))
                                .clicked()
                            {
                                self.stackbar_config.label = option;
                                komorebi_client::send_message(&SocketMessage::StackbarLabel(
                                    self.stackbar_config.label,
                                ))
                                .unwrap();
                            }
                        }
                    });

                    ui.collapsing("Colours", |ui| {
                        ui.collapsing("Focused Text", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.stackbar_config.focused_text_colour,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(
                                    &SocketMessage::StackbarFocusedTextColour(
                                        self.stackbar_config.focused_text_colour.r() as u32,
                                        self.stackbar_config.focused_text_colour.g() as u32,
                                        self.stackbar_config.focused_text_colour.b() as u32,
                                    ),
                                )
                                .unwrap();
                            }
                        });

                        ui.collapsing("Unfocused Text", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.stackbar_config.unfocused_text_colour,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(
                                    &SocketMessage::StackbarUnfocusedTextColour(
                                        self.stackbar_config.unfocused_text_colour.r() as u32,
                                        self.stackbar_config.unfocused_text_colour.g() as u32,
                                        self.stackbar_config.unfocused_text_colour.b() as u32,
                                    ),
                                )
                                .unwrap();
                            }
                        });

                        ui.collapsing("Background", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.stackbar_config.background_colour,
                                Alpha::Opaque,
                            ) {
                                komorebi_client::send_message(
                                    &SocketMessage::StackbarBackgroundColour(
                                        self.stackbar_config.background_colour.r() as u32,
                                        self.stackbar_config.background_colour.g() as u32,
                                        self.stackbar_config.background_colour.b() as u32,
                                    ),
                                )
                                .unwrap();
                            }
                        })
                    });

                    ui.collapsing("Width", |ui| {
                        if ui
                            .add(egui::Slider::new(&mut self.stackbar_config.width, 0..=500))
                            .drag_stopped()
                        {
                            komorebi_client::send_message(&SocketMessage::StackbarTabWidth(
                                self.stackbar_config.width,
                            ))
                            .unwrap();

                            komorebi_client::send_message(&SocketMessage::Retile).unwrap()
                        };
                    });

                    ui.collapsing("Height", |ui| {
                        if ui
                            .add(egui::Slider::new(&mut self.stackbar_config.height, 0..=100))
                            .drag_stopped()
                        {
                            komorebi_client::send_message(&SocketMessage::StackbarHeight(
                                self.stackbar_config.height,
                            ))
                            .unwrap();

                            komorebi_client::send_message(&SocketMessage::Retile).unwrap()
                        };
                    });
                });

                for (monitor_idx, monitor) in self.monitors.iter_mut().enumerate() {
                    ui.collapsing(
                        format!(
                            "Monitor {monitor_idx} ({}x{})",
                            monitor.size.right, monitor.size.bottom
                        ),
                        |ui| {
                            ui.collapsing("Work Area Offset", |ui| {
                                if ui
                                    .add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.left,
                                            0..=500,
                                        )
                                        .text("Left"),
                                    )
                                    .drag_stopped()
                                {
                                    komorebi_client::send_message(
                                        &SocketMessage::MonitorWorkAreaOffset(
                                            monitor_idx,
                                            monitor.work_area_offset,
                                        ),
                                    )
                                    .unwrap();
                                };

                                if ui
                                    .add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.top,
                                            0..=500,
                                        )
                                        .text("Top"),
                                    )
                                    .drag_stopped()
                                {
                                    komorebi_client::send_message(
                                        &SocketMessage::MonitorWorkAreaOffset(
                                            monitor_idx,
                                            monitor.work_area_offset,
                                        ),
                                    )
                                    .unwrap();
                                };

                                if ui
                                    .add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.right,
                                            0..=500,
                                        )
                                        .text("Right"),
                                    )
                                    .drag_stopped()
                                {
                                    komorebi_client::send_message(
                                        &SocketMessage::MonitorWorkAreaOffset(
                                            monitor_idx,
                                            monitor.work_area_offset,
                                        ),
                                    )
                                    .unwrap();
                                };

                                if ui
                                    .add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.bottom,
                                            0..=500,
                                        )
                                        .text("Bottom"),
                                    )
                                    .drag_stopped()
                                {
                                    komorebi_client::send_message(
                                        &SocketMessage::MonitorWorkAreaOffset(
                                            monitor_idx,
                                            monitor.work_area_offset,
                                        ),
                                    )
                                    .unwrap();
                                };
                            });

                            ui.collapsing("Workspaces", |ui| {
                                for (workspace_idx, workspace) in
                                    monitor.workspaces.iter_mut().enumerate()
                                {
                                    ui.collapsing(
                                        format!("Workspace {workspace_idx} ({})", workspace.name),
                                        |ui| {
                                            if ui.button("Focus").clicked() {
                                                komorebi_client::send_message(
                                                    &SocketMessage::MouseFollowsFocus(false),
                                                )
                                                .unwrap();

                                                komorebi_client::send_message(
                                                    &SocketMessage::FocusMonitorWorkspaceNumber(
                                                        monitor_idx,
                                                        workspace_idx,
                                                    ),
                                                )
                                                .unwrap();

                                                komorebi_client::send_message(
                                                    &SocketMessage::MouseFollowsFocus(
                                                        self.mouse_follows_focus,
                                                    ),
                                                )
                                                .unwrap();
                                            }

                                            if ui
                                                .toggle_value(&mut workspace.tile, "Tiling")
                                                .changed()
                                            {
                                                komorebi_client::send_message(
                                                    &SocketMessage::WorkspaceTiling(
                                                        monitor_idx,
                                                        workspace_idx,
                                                        workspace.tile,
                                                    ),
                                                )
                                                .unwrap();
                                            }

                                            ui.collapsing("Name", |ui| {
                                                let monitor_workspaces = self
                                                    .workspace_names
                                                    .get_mut(&monitor_idx)
                                                    .unwrap();
                                                let workspace_name =
                                                    &mut monitor_workspaces[workspace_idx];
                                                if ui
                                                    .text_edit_singleline(workspace_name)
                                                    .lost_focus()
                                                {
                                                    workspace.name.clone_from(workspace_name);
                                                    komorebi_client::send_message(
                                                        &SocketMessage::WorkspaceName(
                                                            monitor_idx,
                                                            workspace_idx,
                                                            workspace.name.clone(),
                                                        ),
                                                    )
                                                    .unwrap();
                                                }
                                            });

                                            ui.collapsing("Layout", |ui| {
                                                for option in [
                                                    DefaultLayout::BSP,
                                                    DefaultLayout::Columns,
                                                    DefaultLayout::Rows,
                                                    DefaultLayout::VerticalStack,
                                                    DefaultLayout::HorizontalStack,
                                                    DefaultLayout::UltrawideVerticalStack,
                                                    DefaultLayout::Grid,
                                                ] {
                                                    if ui
                                                        .add(egui::SelectableLabel::new(
                                                            workspace.layout == option,
                                                            option.to_string(),
                                                        ))
                                                        .clicked()
                                                    {
                                                        workspace.layout = option;
                                                        komorebi_client::send_message(
                                                            &SocketMessage::WorkspaceLayout(
                                                                monitor_idx,
                                                                workspace_idx,
                                                                workspace.layout,
                                                            ),
                                                        )
                                                        .unwrap();
                                                    }
                                                }
                                            });

                                            ui.collapsing("Container Padding", |ui| {
                                                if ui
                                                    .add(egui::Slider::new(
                                                        &mut workspace.container_padding,
                                                        0..=100,
                                                    ))
                                                    .drag_stopped()
                                                {
                                                    komorebi_client::send_message(
                                                        &SocketMessage::ContainerPadding(
                                                            monitor_idx,
                                                            workspace_idx,
                                                            workspace.container_padding,
                                                        ),
                                                    )
                                                    .unwrap();
                                                };
                                            });

                                            ui.collapsing("Workspace Padding", |ui| {
                                                if ui
                                                    .add(egui::Slider::new(
                                                        &mut workspace.workspace_padding,
                                                        0..=100,
                                                    ))
                                                    .drag_stopped()
                                                {
                                                    komorebi_client::send_message(
                                                        &SocketMessage::WorkspacePadding(
                                                            monitor_idx,
                                                            workspace_idx,
                                                            workspace.workspace_padding,
                                                        ),
                                                    )
                                                    .unwrap();
                                                };
                                            });
                                        },
                                    );
                                }
                            });
                        },
                    );
                }
            });
        });
    }
}
