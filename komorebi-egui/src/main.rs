use eframe::egui;
use eframe::egui::color_picker::Alpha;
use eframe::egui::Color32;
use eframe::egui::WindowLevel;
use komorebi_client::ActiveWindowBorderStyle;
use komorebi_client::Colour;
use komorebi_client::DefaultLayout;
use komorebi_client::Layout;
use komorebi_client::Monitor;
use komorebi_client::Rect;
use komorebi_client::Rgb;
use komorebi_client::RuleDebug;
use komorebi_client::SocketMessage;
use komorebi_client::StackbarMode;
use komorebi_client::Window;
use komorebi_client::WindowKind;
use komorebi_client::Workspace;
use random_word::Lang;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

fn main() {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size([320.0, 500.0]),
        follow_system_theme: true,
        ..Default::default()
    };

    eframe::run_native(
        "komorebi-egui",
        native_options,
        Box::new(|cc| Box::new(KomorebiEgui::new(cc))),
    )
    .unwrap();
}

struct KomorebiEgui {
    monitors: Vec<MonitorConfig>,
    border_config: BorderConfig,
    stackbar_config: StackbarConfig,
    mouse_follows_focus: bool,
    hwnd_lookup: isize,
    hwnd_lookup_windows: Vec<Window>,
    hwnd_rule_debug: Option<RuleDebug>,
}

fn colour32(colour: Colour) -> Color32 {
    match colour {
        Colour::Rgb(rgb) => Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8),
        Colour::Hex(hex) => {
            let rgb = Rgb::from(hex);
            Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8)
        }
    }
}

impl KomorebiEgui {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.

        let mut state = serde_json::from_str::<komorebi_client::State>(
            &komorebi_client::send_query(&SocketMessage::State).unwrap(),
        )
        .unwrap();

        let global_state = serde_json::from_str::<komorebi_client::GlobalState>(
            &komorebi_client::send_query(&SocketMessage::GlobalState).unwrap(),
        )
        .unwrap();

        let mut monitors = vec![];
        for m in state.monitors.elements_mut() {
            monitors.push(MonitorConfig::from(m.clone()));
        }

        let border_config = BorderConfig {
            active_window_border_enabled: global_state.active_window_border_enabled,
            active_window_border_style: global_state.active_window_border_style,
            border_width: global_state.border_width,
            border_offset: global_state.border_offset,
            single: colour32(global_state.active_window_border_colours.single),
            stack: colour32(global_state.active_window_border_colours.stack),
            monocle: colour32(global_state.active_window_border_colours.monocle),
        };

        let stackbar_config = StackbarConfig {
            stackbar_mode: global_state.stackbar_mode,
            stackbar_focused_text_colour: colour32(global_state.stackbar_focused_text_colour),
            stackbar_unfocused_text_colour: colour32(global_state.stackbar_unfocused_text_colour),
            stackbar_tab_background_colour: colour32(global_state.stackbar_tab_background_colour),
            stackbar_tab_width: global_state.stackbar_tab_width,
            stackbar_height: global_state.stackbar_height,
        };

        let mut hwnd_lookup_windows = vec![];

        unsafe {
            EnumWindows(
                Some(enum_window),
                windows::Win32::Foundation::LPARAM(
                    &mut hwnd_lookup_windows as *mut Vec<Window> as isize,
                ),
            )
            .unwrap();
        };

        Self {
            monitors,
            border_config,
            stackbar_config,
            mouse_follows_focus: state.mouse_follows_focus,
            hwnd_lookup: 0,
            hwnd_lookup_windows,
            hwnd_rule_debug: None,
        }
    }
}

struct BorderConfig {
    active_window_border_enabled: bool,
    active_window_border_style: ActiveWindowBorderStyle,
    border_width: i32,
    border_offset: i32,
    single: Color32,
    monocle: Color32,
    stack: Color32,
}

struct StackbarConfig {
    stackbar_mode: StackbarMode,
    stackbar_focused_text_colour: Color32,
    stackbar_unfocused_text_colour: Color32,
    stackbar_tab_background_colour: Color32,
    stackbar_tab_width: i32,
    stackbar_height: i32,
}

#[derive(Clone)]
struct MonitorConfig {
    work_area_offset: Rect,
    size: Rect,
    workspaces: Vec<WorkspaceConfig>,
}

impl From<Monitor> for MonitorConfig {
    fn from(value: Monitor) -> Self {
        let mut workspaces = vec![];

        for ws in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(ws.clone()));
        }

        Self {
            work_area_offset: value.work_area_offset().unwrap_or_default(),
            size: *value.size(),
            workspaces,
        }
    }
}

#[derive(Clone)]
struct WorkspaceConfig {
    container_padding: i32,
    workspace_padding: i32,
    layout: DefaultLayout,
    name: String,
}

impl From<Workspace> for WorkspaceConfig {
    fn from(value: Workspace) -> Self {
        Self {
            container_padding: value.container_padding().unwrap_or(20),
            workspace_padding: value.workspace_padding().unwrap_or(20),
            layout: match value.layout() {
                Layout::Default(layout) => *layout,
                Layout::Custom(_) => DefaultLayout::BSP,
            },
            name: value
                .name()
                .clone()
                .unwrap_or_else(|| random_word::gen(Lang::En).to_string()),
        }
    }
}

extern "system" fn enum_window(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<Window>) };
    let window = Window { hwnd: hwnd.0 };

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
    let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx());
    egui_extras::syntax_highlighting::code_view_ui(ui, &theme, code, language);
}
impl eframe::App for KomorebiEgui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_pixels_per_point(1.5);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.set_width(ctx.input(|i| i.viewport().inner_rect.unwrap().width()));

                    ui.collapsing("Debug Windows and Rules", |ui| {
                        let window = Window {
                            hwnd: self.hwnd_lookup,
                        };

                        let title = if let (Ok(title), Ok(exe)) = (window.title(), window.exe()) {
                            format!("{} - {:?} - {:?}", window.hwnd, exe, title)
                        } else {
                            String::from("Select a Window")
                        };

                        if ui.button("Refresh Window List").clicked() {
                            let mut windows = vec![];
                            unsafe {
                                EnumWindows(
                                    Some(enum_window),
                                    windows::Win32::Foundation::LPARAM(
                                        &mut windows as *mut Vec<Window> as isize,
                                    ),
                                )
                                .unwrap();
                            };

                            self.hwnd_lookup_windows = windows;
                        }

                        egui::ComboBox::from_label("Select one!")
                            .selected_text(format!("{:?}", title))
                            .show_ui(ui, |ui| {
                                for w in &self.hwnd_lookup_windows {
                                    if ui
                                        .selectable_value(
                                            &mut self.hwnd_lookup,
                                            w.hwnd,
                                            format!(
                                                "{} - {:?} - {:?}",
                                                w.hwnd,
                                                w.exe().unwrap(),
                                                w.title().unwrap()
                                            ),
                                        )
                                        .changed()
                                    {
                                        let response = komorebi_client::send_query(
                                            &SocketMessage::DebugWindow(w.hwnd),
                                        )
                                        .unwrap();

                                        let debug: RuleDebug =
                                            serde_json::from_str(&response).unwrap();

                                        self.hwnd_rule_debug = Some(debug);
                                    };
                                }
                            });
                    });

                    if let Some(debug) = &self.hwnd_rule_debug {
                        ui.horizontal(|ui| {
                            json_view_ui(ui, &serde_json::to_string_pretty(&debug).unwrap());
                        });
                    }

                    ui.separator();

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

                    ui.collapsing("Borders", |ui| {
                        if ui
                            .toggle_value(
                                &mut self.border_config.active_window_border_enabled,
                                "Active Window Border",
                            )
                            .changed()
                        {
                            komorebi_client::send_message(&SocketMessage::ActiveWindowBorder(
                                self.border_config.active_window_border_enabled,
                            ))
                            .unwrap();
                        }

                        ui.collapsing("Style", |ui| {
                            for option in [
                                ActiveWindowBorderStyle::System,
                                ActiveWindowBorderStyle::Rounded,
                                ActiveWindowBorderStyle::Square,
                            ] {
                                if ui
                                    .add(egui::SelectableLabel::new(
                                        option == self.border_config.active_window_border_style,
                                        option.to_string(),
                                    ))
                                    .clicked()
                                {
                                    komorebi_client::send_message(
                                        &SocketMessage::ActiveWindowBorderStyle(option),
                                    )
                                    .unwrap();

                                    self.border_config.active_window_border_style = option;
                                }
                            }
                        });

                        ui.collapsing("Width", |ui| {
                            if ui
                                .add(egui::Slider::new(
                                    &mut self.border_config.border_width,
                                    -10..=30,
                                ))
                                .drag_stopped()
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
                                    -10..=30,
                                ))
                                .drag_stopped()
                            {
                                komorebi_client::send_message(&SocketMessage::BorderOffset(
                                    self.border_config.border_offset,
                                ))
                                .unwrap();
                            };
                        });

                        ui.collapsing("Colours", |ui| {
                            ui.collapsing("Single", |ui| {
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut self.border_config.single,
                                    Alpha::Opaque,
                                ) {
                                    komorebi_client::send_message(
                                        &SocketMessage::ActiveWindowBorderColour(
                                            WindowKind::Single,
                                            self.border_config.single.r() as u32,
                                            self.border_config.single.g() as u32,
                                            self.border_config.single.b() as u32,
                                        ),
                                    )
                                    .unwrap();
                                }
                            });

                            ui.collapsing("Monocle", |ui| {
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut self.border_config.monocle,
                                    Alpha::Opaque,
                                ) {
                                    komorebi_client::send_message(
                                        &SocketMessage::ActiveWindowBorderColour(
                                            WindowKind::Single,
                                            self.border_config.monocle.r() as u32,
                                            self.border_config.monocle.g() as u32,
                                            self.border_config.monocle.b() as u32,
                                        ),
                                    )
                                    .unwrap();
                                }
                            });

                            ui.collapsing("Stack", |ui| {
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut self.border_config.stack,
                                    Alpha::Opaque,
                                ) {
                                    komorebi_client::send_message(
                                        &SocketMessage::ActiveWindowBorderColour(
                                            WindowKind::Single,
                                            self.border_config.stack.r() as u32,
                                            self.border_config.stack.g() as u32,
                                            self.border_config.stack.b() as u32,
                                        ),
                                    )
                                    .unwrap();
                                }
                            });
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
                                    option == self.stackbar_config.stackbar_mode,
                                    option.to_string(),
                                ))
                                .clicked()
                            {
                                komorebi_client::send_message(&SocketMessage::StackbarMode(option))
                                    .unwrap();

                                self.stackbar_config.stackbar_mode = option;

                                komorebi_client::send_message(&SocketMessage::Retile).unwrap();
                            }
                        }

                        ui.collapsing("Width", |ui| {
                            if ui
                                .add(egui::Slider::new(
                                    &mut self.stackbar_config.stackbar_tab_width,
                                    0..=600,
                                ))
                                .drag_stopped()
                            {
                                komorebi_client::send_message(&SocketMessage::StackbarTabWidth(
                                    self.stackbar_config.stackbar_tab_width,
                                ))
                                .unwrap();
                            };
                        });

                        ui.collapsing("Height", |ui| {
                            if ui
                                .add(egui::Slider::new(
                                    &mut self.stackbar_config.stackbar_height,
                                    0..=50,
                                ))
                                .drag_stopped()
                            {
                                komorebi_client::send_message(&SocketMessage::StackbarHeight(
                                    self.stackbar_config.stackbar_height,
                                ))
                                .unwrap();
                            };
                        });

                        ui.collapsing("Colours", |ui| {
                            ui.collapsing("Focused Text", |ui| {
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut self.stackbar_config.stackbar_focused_text_colour,
                                    Alpha::Opaque,
                                ) {
                                    komorebi_client::send_message(
                                        &SocketMessage::StackbarFocusedTextColour(
                                            self.stackbar_config.stackbar_focused_text_colour.r()
                                                as u32,
                                            self.stackbar_config.stackbar_focused_text_colour.g()
                                                as u32,
                                            self.stackbar_config.stackbar_focused_text_colour.b()
                                                as u32,
                                        ),
                                    )
                                    .unwrap();
                                }
                            });

                            ui.collapsing("Unfocused Text", |ui| {
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut self.stackbar_config.stackbar_unfocused_text_colour,
                                    Alpha::Opaque,
                                ) {
                                    komorebi_client::send_message(
                                        &SocketMessage::StackbarUnfocusedTextColour(
                                            self.stackbar_config.stackbar_unfocused_text_colour.r()
                                                as u32,
                                            self.stackbar_config.stackbar_unfocused_text_colour.g()
                                                as u32,
                                            self.stackbar_config.stackbar_unfocused_text_colour.b()
                                                as u32,
                                        ),
                                    )
                                    .unwrap();
                                }
                            });

                            ui.collapsing("Tab Background", |ui| {
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut self.stackbar_config.stackbar_tab_background_colour,
                                    Alpha::Opaque,
                                ) {
                                    komorebi_client::send_message(
                                        &SocketMessage::StackbarBackgroundColour(
                                            self.stackbar_config.stackbar_tab_background_colour.r()
                                                as u32,
                                            self.stackbar_config.stackbar_tab_background_colour.g()
                                                as u32,
                                            self.stackbar_config.stackbar_tab_background_colour.b()
                                                as u32,
                                        ),
                                    )
                                    .unwrap();
                                }
                            });
                        });
                    });

                    for (monitor_idx, monitor) in self.monitors.iter_mut().enumerate() {
                        ui.collapsing(
                            format!(
                                "Monitor {monitor_idx} ({} x {})",
                                monitor.size.right, monitor.size.bottom
                            ),
                            |ui| {
                                ui.collapsing("Work Area Offset", |ui| {
                                    if ui
                                        .add(
                                            egui::Slider::new(
                                                &mut monitor.work_area_offset.left,
                                                0..=1000,
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
                                                0..=1000,
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
                                                0..=1000,
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
                                                0..=1000,
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

                                            ui.collapsing("Name", |ui| {
                                                if ui
                                                    .text_edit_singleline(&mut workspace.name)
                                                    .lost_focus()
                                                {
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
                                                            option == workspace.layout,
                                                            option.to_string(),
                                                        ))
                                                        .clicked()
                                                    {
                                                        komorebi_client::send_message(
                                                            &SocketMessage::WorkspaceLayout(
                                                                monitor_idx,
                                                                workspace_idx,
                                                                option,
                                                            ),
                                                        )
                                                        .unwrap();

                                                        workspace.layout = option;
                                                    }
                                                }
                                            });

                                            ui.collapsing("Container Padding", |ui| {
                                                if ui
                                                    .add(
                                                        egui::Slider::new(
                                                            &mut workspace.container_padding,
                                                            -100..=100,
                                                        )
                                                        .text("Container Padding"),
                                                    )
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

                                                    komorebi_client::send_message(
                                                        &SocketMessage::Retile,
                                                    )
                                                    .unwrap();
                                                };
                                            });

                                            ui.collapsing("Workspace Padding", |ui| {
                                                if ui
                                                    .add(
                                                        egui::Slider::new(
                                                            &mut workspace.workspace_padding,
                                                            -100..=100,
                                                        )
                                                        .text("Workspace Padding"),
                                                    )
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

                                                    komorebi_client::send_message(
                                                        &SocketMessage::Retile,
                                                    )
                                                    .unwrap();
                                                };
                                            });
                                        },
                                    );
                                }
                            },
                        );
                    }
                });
            });
        });
    }
}
