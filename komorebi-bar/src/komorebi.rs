use crate::bar::apply_theme;
use crate::config::DisplayFormat;
use crate::config::KomobarTheme;
use crate::komorebi_layout::KomorebiLayout;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::ui::CustomUi;
use crate::widget::BarWidget;
use crate::MAX_LABEL_WIDTH;
use crate::MONITOR_INDEX;
use crossbeam_channel::Receiver;
use crossbeam_channel::TryRecvError;
use eframe::egui::Color32;
use eframe::egui::ColorImage;
use eframe::egui::Context;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::TextureHandle;
use eframe::egui::TextureOptions;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use image::RgbaImage;
use komorebi_client::Container;
use komorebi_client::NotificationEvent;
use komorebi_client::Rect;
use komorebi_client::SocketMessage;
use komorebi_client::Window;
use komorebi_client::Workspace;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;

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
    /// Display format of the workspace
    pub display: Option<DisplayFormat>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiLayoutConfig {
    /// Enable the Komorebi Layout widget
    pub enable: bool,
    /// List of layout options
    pub options: Option<Vec<KomorebiLayout>>,
    /// Display format of the current layout
    pub display: Option<DisplayFormat>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KomorebiFocusedWindowConfig {
    /// Enable the Komorebi Focused Window widget
    pub enable: bool,
    /// DEPRECATED: use 'display' instead (Show the icon of the currently focused window)
    pub show_icon: Option<bool>,
    /// Display format of the currently focused window
    pub display: Option<DisplayFormat>,
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
                layout: KomorebiLayout::Default(komorebi_client::DefaultLayout::BSP),
                workspaces: vec![],
                hide_empty_workspaces: value.workspaces.hide_empty_workspaces,
                mouse_follows_focus: true,
                work_area_offset: None,
                focused_container_information: KomorebiNotificationStateContainerInformation::EMPTY,
                stack_accent: None,
                monitor_index: MONITOR_INDEX.load(Ordering::SeqCst),
            })),
            workspaces: value.workspaces,
            layout: value.layout.clone(),
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
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let mut komorebi_notification_state = self.komorebi_notification_state.borrow_mut();

        if self.workspaces.enable {
            let mut update = None;

            if !komorebi_notification_state.workspaces.is_empty() {
                let format = self.workspaces.display.unwrap_or(DisplayFormat::Text);

                config.apply_on_widget(false, ui, |ui| {
                    for (i, (ws, container_information)) in
                        komorebi_notification_state.workspaces.iter().enumerate()
                    {
                        if SelectableFrame::new(
                            komorebi_notification_state.selected_workspace.eq(ws),
                        )
                        .show(ui, |ui| {
                            let mut response = None;

                            if let DisplayFormat::Icon | DisplayFormat::IconAndText = format {
                                if let Some(container_information) = container_information {
                                    for icon in container_information.icons.iter().flatten() {
                                        response = Some(
                                            ui.add(
                                                Image::from(&img_to_texture(ctx, icon))
                                                    .maintain_aspect_ratio(true)
                                                    .shrink_to_fit(),
                                            ),
                                        );
                                    }
                                }
                            }

                            if match format {
                                DisplayFormat::Icon => response.is_none(),
                                _ => false,
                            } {
                                ui.add(
                                    Label::new(egui_phosphor::regular::PLACEHOLDER.to_string())
                                        .selectable(false),
                                )
                            } else if match format {
                                DisplayFormat::Icon => response.is_some(),
                                _ => false,
                            } {
                                response.unwrap()
                            } else {
                                ui.add(Label::new(ws.to_string()).selectable(false))
                            }
                        })
                        .clicked()
                        {
                            update = Some(ws.to_string());
                            let mut proceed = true;

                            if komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                                false,
                            ))
                            .is_err()
                            {
                                tracing::error!(
                                    "could not send message to komorebi: MouseFollowsFocus"
                                );
                                proceed = false;
                            }

                            if proceed
                                && komorebi_client::send_message(
                                    &SocketMessage::FocusMonitorWorkspaceNumber(
                                        komorebi_notification_state.monitor_index,
                                        i,
                                    ),
                                )
                                .is_err()
                            {
                                tracing::error!(
                                    "could not send message to komorebi: FocusWorkspaceNumber"
                                );
                                proceed = false;
                            }

                            if proceed
                                && komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                                    komorebi_notification_state.mouse_follows_focus,
                                ))
                                .is_err()
                            {
                                tracing::error!(
                                    "could not send message to komorebi: MouseFollowsFocus"
                                );
                                proceed = false;
                            }

                            if proceed
                                && komorebi_client::send_message(
                                    &SocketMessage::RetileWithResizeDimensions,
                                )
                                .is_err()
                            {
                                tracing::error!("could not send message to komorebi: Retile");
                            }
                        }
                    }
                });
            }

            if let Some(update) = update {
                komorebi_notification_state.selected_workspace = update;
            }
        }

        if let Some(layout_config) = &self.layout {
            if layout_config.enable {
                let workspace_idx: Option<usize> = komorebi_notification_state
                    .workspaces
                    .iter()
                    .position(|o| komorebi_notification_state.selected_workspace.eq(&o.0));

                komorebi_notification_state.layout.show(
                    ctx,
                    ui,
                    config,
                    layout_config,
                    workspace_idx,
                );
            }
        }

        if let Some(configuration_switcher) = &self.configuration_switcher {
            if configuration_switcher.enable {
                for (name, location) in configuration_switcher.configurations.iter() {
                    let path = PathBuf::from(location);
                    if path.is_file() {
                        config.apply_on_widget(false, ui,|ui|{
                    if SelectableFrame::new(false).show(ui, |ui|{
                          ui.add(Label::new(name).selectable(false))
                            })
                            .clicked()
                        {
                            let canonicalized = dunce::canonicalize(path.clone()).unwrap_or(path);
                            let mut proceed = true;
                            if komorebi_client::send_message(&SocketMessage::ReplaceConfiguration(
                                canonicalized,
                            ))
                            .is_err()
                            {
                                tracing::error!(
                                    "could not send message to komorebi: ReplaceConfiguration"
                                );
                                proceed = false;
                            }

                            if let Some(rect) = komorebi_notification_state.work_area_offset {
                                if proceed {
                                    match komorebi_client::send_query(&SocketMessage::Query(
                                        komorebi_client::StateQuery::FocusedMonitorIndex,
                                    )) {
                                        Ok(idx) => {
                                            if let Ok(monitor_idx) = idx.parse::<usize>() {
                                                if komorebi_client::send_message(
                                                    &SocketMessage::MonitorWorkAreaOffset(
                                                        monitor_idx,
                                                        rect,
                                                    ),
                                                )
                                                .is_err()
                                                {
                                                    tracing::error!(
                                                    "could not send message to komorebi: MonitorWorkAreaOffset"
                                                );
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            tracing::error!(
                                                "could not send message to komorebi: Query"
                                            );
                                        }
                                    }
                                }
                            }
                        }});
                    }
                }
            }
        }

        if let Some(focused_window) = self.focused_window {
            if focused_window.enable {
                let titles = &komorebi_notification_state
                    .focused_container_information
                    .titles;
                if !titles.is_empty() {
                    config.apply_on_widget(false, ui, |ui| {
                        let icons = &komorebi_notification_state
                            .focused_container_information
                            .icons;
                        let focused_window_idx = komorebi_notification_state
                            .focused_container_information
                            .focused_window_idx;

                        let iter = titles.iter().zip(icons.iter());

                        for (i, (title, icon)) in iter.enumerate() {
                            let selected = i == focused_window_idx;

                            if SelectableFrame::new(selected)
                                .show(ui, |ui| {
                                    // handle legacy setting
                                    let format = focused_window.display.unwrap_or(
                                        if focused_window.show_icon.unwrap_or(false) {
                                            DisplayFormat::IconAndText
                                        } else {
                                            DisplayFormat::Text
                                        },
                                    );

                                    if let DisplayFormat::Icon | DisplayFormat::IconAndText = format
                                    {
                                        if let Some(img) = icon {
                                            let response = ui.add(
                                                Image::from(&img_to_texture(ctx, img))
                                                    .maintain_aspect_ratio(true)
                                                    .shrink_to_fit(),
                                            );

                                            if let DisplayFormat::Icon = format {
                                                response.on_hover_text(title);
                                            }
                                        }
                                    }

                                    if let DisplayFormat::Text | DisplayFormat::IconAndText = format
                                    {
                                        let available_height = ui.available_height();
                                        let mut custom_ui = CustomUi(ui);

                                        custom_ui.add_sized_left_to_right(
                                            Vec2::new(
                                                MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32,
                                                available_height,
                                            ),
                                            Label::new(title).selectable(false).truncate(),
                                        );
                                    }
                                })
                                .clicked()
                            {
                                if selected {
                                    return;
                                }

                                if komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                                    false,
                                ))
                                .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: MouseFollowsFocus"
                                    );
                                }

                                if komorebi_client::send_message(&SocketMessage::FocusStackWindow(
                                    i,
                                ))
                                .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: FocusStackWindow"
                                    );
                                }

                                if komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                                    komorebi_notification_state.mouse_follows_focus,
                                ))
                                .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: MouseFollowsFocus"
                                    );
                                }
                            }
                        }
                    });
                }
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
    pub workspaces: Vec<(
        String,
        Option<KomorebiNotificationStateContainerInformation>,
    )>,
    pub selected_workspace: String,
    pub focused_container_information: KomorebiNotificationStateContainerInformation,
    pub layout: KomorebiLayout,
    pub hide_empty_workspaces: bool,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    pub stack_accent: Option<Color32>,
    pub monitor_index: usize,
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
        match rx_gui.try_recv() {
            Err(error) => match error {
                TryRecvError::Empty => {}
                TryRecvError::Disconnected => {
                    tracing::error!(
                        "failed to receive komorebi notification on gui thread: {error}"
                    );
                }
            },
            Ok(notification) => {
                match notification.event {
                    NotificationEvent::WindowManager(_) => {}
                    NotificationEvent::Socket(message) => match message {
                        SocketMessage::ReloadStaticConfiguration(path) => {
                            if let Ok(config) = komorebi_client::StaticConfig::read(&path) {
                                if let Some(theme) = config.theme {
                                    apply_theme(ctx, KomobarTheme::from(theme), bg_color.clone());
                                    tracing::info!("applied theme from updated komorebi.json");
                                }
                            }
                        }
                        SocketMessage::Theme(theme) => {
                            apply_theme(ctx, KomobarTheme::from(theme), bg_color);
                            tracing::info!("applied theme from komorebi socket message");
                        }
                        _ => {}
                    },
                }

                self.monitor_index = monitor_index;

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
                    let should_show = if self.hide_empty_workspaces {
                        focused_workspace_idx == i || !ws.containers().is_empty()
                    } else {
                        true
                    };

                    if should_show {
                        workspaces.push((
                            ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)),
                            Some(ws.into()),
                        ));
                    }
                }

                self.workspaces = workspaces;

                if monitor.workspaces()[focused_workspace_idx]
                    .monocle_container()
                    .is_some()
                {
                    self.layout = KomorebiLayout::Monocle;
                } else if !*monitor.workspaces()[focused_workspace_idx].tile() {
                    self.layout = KomorebiLayout::Floating;
                } else if notification.state.is_paused {
                    self.layout = KomorebiLayout::Paused;
                } else {
                    self.layout = match monitor.workspaces()[focused_workspace_idx].layout() {
                        komorebi_client::Layout::Default(layout) => {
                            KomorebiLayout::Default(*layout)
                        }
                        komorebi_client::Layout::Custom(_) => KomorebiLayout::Custom,
                    };
                }

                self.focused_container_information =
                    (&monitor.workspaces()[focused_workspace_idx]).into();
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct KomorebiNotificationStateContainerInformation {
    pub titles: Vec<String>,
    pub icons: Vec<Option<RgbaImage>>,
    pub focused_window_idx: usize,
}

impl From<&Workspace> for KomorebiNotificationStateContainerInformation {
    fn from(value: &Workspace) -> Self {
        let mut container_info = Self::EMPTY;

        if let Some(container) = value.monocle_container() {
            container_info = container.into();
        } else if let Some(container) = value.focused_container() {
            container_info = container.into();
        }

        for floating_window in value.floating_windows() {
            if floating_window.is_focused() {
                container_info = floating_window.into();
            }
        }

        container_info
    }
}

impl From<&Container> for KomorebiNotificationStateContainerInformation {
    fn from(value: &Container) -> Self {
        Self {
            titles: value
                .windows()
                .iter()
                .map(|w| w.title().unwrap_or_default())
                .collect::<Vec<_>>(),
            icons: value
                .windows()
                .iter()
                .map(|w| windows_icons::get_icon_by_process_id(w.process_id()))
                .collect::<Vec<_>>(),
            focused_window_idx: value.focused_window_idx(),
        }
    }
}

impl From<&Window> for KomorebiNotificationStateContainerInformation {
    fn from(value: &Window) -> Self {
        Self {
            titles: vec![value.title().unwrap_or_default()],
            icons: vec![windows_icons::get_icon_by_process_id(value.process_id())],
            focused_window_idx: 0,
        }
    }
}

impl KomorebiNotificationStateContainerInformation {
    pub const EMPTY: Self = Self {
        titles: vec![],
        icons: vec![],
        focused_window_idx: 0,
    };
}
