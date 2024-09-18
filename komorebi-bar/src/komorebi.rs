use crate::bar::apply_theme;
use crate::config::KomobarTheme;
use crate::widget::BarWidget;
use crate::WIDGET_SPACING;
use crossbeam_channel::Receiver;
use eframe::egui::Color32;
use eframe::egui::ColorImage;
use eframe::egui::Context;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::SelectableLabel;
use eframe::egui::Sense;
use eframe::egui::TextureHandle;
use eframe::egui::TextureOptions;
use eframe::egui::Ui;
use image::RgbaImage;
use komorebi_client::CycleDirection;
use komorebi_client::NotificationEvent;
use komorebi_client::Rect;
use komorebi_client::SocketMessage;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiConfig {
    /// Configure the Workspaces widget
    pub workspaces: KomorebiWorkspacesConfig,
    /// Configure the Layout widget
    pub layout: Option<KomorebiLayoutConfig>,
    /// Configure the Focused Window widget
    pub focused_window: Option<KomorebiFocusedWindowConfig>,
    /// Configure the Configuration Switcher widget
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiWorkspacesConfig {
    /// Enable the Komorebi Workspaces widget
    pub enable: bool,
    /// Hide workspaces without any windows
    pub hide_empty_workspaces: bool,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiLayoutConfig {
    /// Enable the Komorebi Layout widget
    pub enable: bool,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiFocusedWindowConfig {
    /// Enable the Komorebi Focused Window widget
    pub enable: bool,
    /// Show the icon of the currently focused window
    pub show_icon: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiConfigurationSwitcherConfig {
    /// Enable the Komorebi Configurations widget
    pub enable: bool,
    /// A map of display friendly name => path to configuration.json
    pub configurations: BTreeMap<String, String>,
}

impl From<&KomorebiConfig> for Komorebi {
    fn from(value: &KomorebiConfig) -> Self {
        let configuration_switcher =
            if let Some(configuration_switcher) = &value.configuration_switcher {
                let mut configuration_switcher = configuration_switcher.clone();
                for (_, location) in configuration_switcher.configurations.iter_mut() {
                    if let Ok(expanded) = std::env::var("KOMOREBI_CONFIG_HOME") {
                        *location = location.replace("$Env:KOMOREBI_CONFIG_HOME", &expanded);
                    }

                    if let Ok(expanded) = std::env::var("USERPROFILE") {
                        *location = location.replace("$Env:USERPROFILE", &expanded);
                    }

                    *location = dunce::simplified(&PathBuf::from(location.clone()))
                        .to_string_lossy()
                        .to_string();
                }
                Some(configuration_switcher)
            } else {
                None
            };

        Self {
            komorebi_notification_state: Rc::new(RefCell::new(KomorebiNotificationState {
                selected_workspace: String::new(),
                focused_window_title: String::new(),
                focused_window_pid: None,
                focused_window_icon: None,
                layout: String::new(),
                workspaces: vec![],
                hide_empty_workspaces: value.workspaces.hide_empty_workspaces,
                mouse_follows_focus: true,
                work_area_offset: None,
            })),
            workspaces: value.workspaces,
            layout: value.layout,
            focused_window: value.focused_window,
            configuration_switcher,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Komorebi {
    pub komorebi_notification_state: Rc<RefCell<KomorebiNotificationState>>,
    pub workspaces: KomorebiWorkspacesConfig,
    pub layout: Option<KomorebiLayoutConfig>,
    pub focused_window: Option<KomorebiFocusedWindowConfig>,
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

impl BarWidget for Komorebi {
    fn render(&mut self, ctx: &Context, ui: &mut Ui) {
        let mut komorebi_notification_state = self.komorebi_notification_state.borrow_mut();

        if self.workspaces.enable {
            let mut update = None;

            for (i, ws) in komorebi_notification_state.workspaces.iter().enumerate() {
                if ui
                    .add(SelectableLabel::new(
                        komorebi_notification_state.selected_workspace.eq(ws),
                        ws.to_string(),
                    ))
                    .clicked()
                {
                    update = Some(ws.to_string());
                    komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(false))
                        .unwrap();
                    komorebi_client::send_message(&SocketMessage::FocusWorkspaceNumber(i)).unwrap();
                    komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                        komorebi_notification_state.mouse_follows_focus,
                    ))
                    .unwrap();
                    komorebi_client::send_message(&SocketMessage::Retile).unwrap();
                }
            }

            if let Some(update) = update {
                komorebi_notification_state.selected_workspace = update;
            }

            ui.add_space(WIDGET_SPACING);
        }

        if let Some(layout) = self.layout {
            if layout.enable {
                if ui
                    .add(
                        Label::new(&komorebi_notification_state.layout)
                            .selectable(false)
                            .sense(Sense::click()),
                    )
                    .clicked()
                {
                    komorebi_client::send_message(&SocketMessage::CycleLayout(
                        CycleDirection::Next,
                    ))
                    .unwrap();
                }

                ui.add_space(WIDGET_SPACING);
            }
        }

        if let Some(configuration_switcher) = &self.configuration_switcher {
            if configuration_switcher.enable {
                for (name, location) in configuration_switcher.configurations.iter() {
                    let path = PathBuf::from(location);
                    if path.is_file()
                        && ui
                            .add(Label::new(name).selectable(false).sense(Sense::click()))
                            .clicked()
                    {
                        let canonicalized = dunce::canonicalize(path.clone()).unwrap_or(path);
                        komorebi_client::send_message(&SocketMessage::ReplaceConfiguration(
                            canonicalized,
                        ))
                        .unwrap();

                        if let Some(rect) = komorebi_notification_state.work_area_offset {
                            let monitor_index = komorebi_client::send_query(&SocketMessage::Query(
                                komorebi_client::StateQuery::FocusedMonitorIndex,
                            ))
                            .unwrap();

                            komorebi_client::send_message(&SocketMessage::MonitorWorkAreaOffset(
                                monitor_index.parse::<usize>().unwrap(),
                                rect,
                            ))
                            .unwrap();
                        }
                    }
                }

                ui.add_space(WIDGET_SPACING);
            }
        }

        if let Some(focused_window) = self.focused_window {
            if focused_window.enable {
                if focused_window.show_icon {
                    if let Some(img) = &komorebi_notification_state.focused_window_icon {
                        ui.add(
                            Image::from(&img_to_texture(ctx, img))
                                .maintain_aspect_ratio(true)
                                .max_height(15.0),
                        );
                    }
                }

                ui.add(
                    Label::new(&komorebi_notification_state.focused_window_title).selectable(false),
                );

                ui.add_space(WIDGET_SPACING);
            }
        }
    }
}

fn img_to_texture(ctx: &Context, rgba_image: &RgbaImage) -> TextureHandle {
    let size = [rgba_image.width() as usize, rgba_image.height() as usize];
    let pixels = rgba_image.as_flat_samples();
    let color_image = ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
    ctx.load_texture("icon", color_image, TextureOptions::default())
}

#[derive(Clone, Debug)]
pub struct KomorebiNotificationState {
    pub workspaces: Vec<String>,
    pub selected_workspace: String,
    pub focused_window_title: String,
    pub focused_window_pid: Option<u32>,
    pub focused_window_icon: Option<RgbaImage>,
    pub layout: String,
    pub hide_empty_workspaces: bool,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
}

impl KomorebiNotificationState {
    pub fn update_from_config(&mut self, config: &Self) {
        self.hide_empty_workspaces = config.hide_empty_workspaces;
    }

    pub fn handle_notification(
        &mut self,
        ctx: &Context,
        monitor_index: usize,
        rx_gui: Receiver<komorebi_client::Notification>,
        bg_color: Rc<RefCell<Color32>>,
    ) {
        if let Ok(notification) = rx_gui.try_recv() {
            if let NotificationEvent::Socket(SocketMessage::ReloadStaticConfiguration(path)) =
                notification.event
            {
                if let Ok(config) = komorebi_client::StaticConfig::read(&path) {
                    if let Some(theme) = config.theme {
                        apply_theme(ctx, KomobarTheme::from(theme), bg_color);
                        tracing::info!("applied theme from updated komorebi.json");
                    }
                }
            }

            self.mouse_follows_focus = notification.state.mouse_follows_focus;

            let monitor = &notification.state.monitors.elements()[monitor_index];
            self.work_area_offset =
                notification.state.monitors.elements()[monitor_index].work_area_offset();

            let focused_workspace_idx = monitor.focused_workspace_idx();

            let mut workspaces = vec![];
            self.selected_workspace = monitor.workspaces()[focused_workspace_idx]
                .name()
                .to_owned()
                .unwrap_or_else(|| format!("{}", focused_workspace_idx + 1));

            for (i, ws) in monitor.workspaces().iter().enumerate() {
                let should_add = if self.hide_empty_workspaces {
                    focused_workspace_idx == i || !ws.containers().is_empty()
                } else {
                    true
                };

                if should_add {
                    workspaces.push(ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)));
                }
            }

            self.workspaces = workspaces;
            self.layout = match monitor.workspaces()[focused_workspace_idx].layout() {
                komorebi_client::Layout::Default(layout) => layout.to_string(),
                komorebi_client::Layout::Custom(_) => String::from("Custom"),
            };

            if let Some(container) = monitor.workspaces()[focused_workspace_idx].monocle_container()
            {
                if let Some(window) = container.focused_window() {
                    if let Ok(title) = window.title() {
                        self.focused_window_title.clone_from(&title);
                        self.focused_window_pid = Some(window.process_id());
                        if let Some(img) =
                            windows_icons::get_icon_by_process_id(window.process_id())
                        {
                            self.focused_window_icon = Some(img);
                        } else {
                            self.focused_window_icon = None;
                        }
                    }
                }
            } else if let Some(container) =
                monitor.workspaces()[focused_workspace_idx].focused_container()
            {
                if let Some(window) = container.focused_window() {
                    if let Ok(title) = window.title() {
                        self.focused_window_title.clone_from(&title);
                        self.focused_window_pid = Some(window.process_id());
                        if let Some(img) =
                            windows_icons::get_icon_by_process_id(window.process_id())
                        {
                            self.focused_window_icon = Some(img);
                        } else {
                            self.focused_window_icon = None;
                        }
                    }
                }
            } else {
                self.focused_window_title.clear();
                self.focused_window_icon = None;
            }

            if let Some(container) = monitor.workspaces()[focused_workspace_idx].monocle_container()
            {
                if let Some(window) = container.focused_window() {
                    if let Ok(title) = window.title() {
                        self.focused_window_title.clone_from(&title);
                    }
                }
            }

            if let Some(window) = monitor.workspaces()[focused_workspace_idx].maximized_window() {
                if let Ok(title) = window.title() {
                    self.focused_window_title.clone_from(&title);
                }
            }
        }
    }
}
