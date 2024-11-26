use crate::bar::apply_theme;
use crate::config::KomobarTheme;
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
use eframe::egui::SelectableLabel;
use eframe::egui::Sense;
use eframe::egui::TextureHandle;
use eframe::egui::TextureOptions;
use eframe::egui::Ui;
use eframe::egui::Vec2;
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
use std::fmt::Display;
use std::fmt::Formatter;
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
                focused_container_information: (vec![], vec![], 0),
                stack_accent: None,
                monitor_index: MONITOR_INDEX.load(Ordering::SeqCst),
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
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let mut komorebi_notification_state = self.komorebi_notification_state.borrow_mut();

        if self.workspaces.enable {
            let mut update = None;

            // NOTE: There should always be at least one workspace if the bar is connected to komorebi.
            config.apply_on_widget(false, ui, |ui| {
                for (i, (ws, should_show)) in
                    komorebi_notification_state.workspaces.iter().enumerate()
                {
                    if *should_show
                        && ui
                            .add(SelectableLabel::new(
                                komorebi_notification_state.selected_workspace.eq(ws),
                                ws.to_string(),
                            ))
                            .clicked()
                    {
                        update = Some(ws.to_string());
                        let mut proceed = true;

                        if komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(false))
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

            if let Some(update) = update {
                komorebi_notification_state.selected_workspace = update;
            }
        }

        if let Some(layout) = self.layout {
            if layout.enable {
                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&eframe::egui::TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(eframe::egui::FontId::default);

                config.apply_on_widget(false, ui, |ui| {
                    if komorebi_notification_state
                        .layout
                        .show(font_id, ctx, ui)
                        .clicked()
                    {
                        //match komorebi_notification_state.layout {
                        //    KomorebiLayout::Default(_) => {
                        //        dbg!("change layout");

                        //        let monitor_idx: usize = 1;
                        //        let workspace_idx: usize = 0;
                        //        let new_layout = komorebi_client::DefaultLayout::BSP;

                        //        if komorebi_client::send_message(&SocketMessage::WorkspaceLayout(
                        //            monitor_idx,
                        //            workspace_idx,
                        //            new_layout,
                        //        ))
                        //        .is_err()
                        //        {
                        //            tracing::error!(
                        //                "could not send message to komorebi: CycleLayout"
                        //            );
                        //        }
                        //    }
                        //    _ => {}
                        //}

                        match komorebi_notification_state.layout {
                            KomorebiLayout::Default(_) => {
                                if komorebi_client::send_message(&SocketMessage::CycleLayout(
                                    CycleDirection::Next,
                                ))
                                .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: CycleLayout"
                                    );
                                }
                            }
                            KomorebiLayout::Floating => {
                                if komorebi_client::send_message(&SocketMessage::ToggleTiling)
                                    .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: ToggleTiling"
                                    );
                                }
                            }
                            KomorebiLayout::Paused => {
                                if komorebi_client::send_message(&SocketMessage::TogglePause)
                                    .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: TogglePause"
                                    );
                                }
                            }
                            KomorebiLayout::Custom => {}
                        }
                    }

                    //if ui
                    //    .add(
                    //        Label::new(komorebi_notification_state.layout.to_string())
                    //            .selectable(false)
                    //            .sense(Sense::click()),
                    //    )
                    //    .clicked()
                    //{
                    //    match komorebi_notification_state.layout {
                    //        KomorebiLayout::Default(_) => {
                    //            if komorebi_client::send_message(&SocketMessage::CycleLayout(
                    //                CycleDirection::Next,
                    //            ))
                    //            .is_err()
                    //            {
                    //                tracing::error!(
                    //                    "could not send message to komorebi: CycleLayout"
                    //                );
                    //            }
                    //        }
                    //        KomorebiLayout::Floating => {
                    //            if komorebi_client::send_message(&SocketMessage::ToggleTiling)
                    //                .is_err()
                    //            {
                    //                tracing::error!(
                    //                    "could not send message to komorebi: ToggleTiling"
                    //                );
                    //            }
                    //        }
                    //        KomorebiLayout::Paused => {
                    //            if komorebi_client::send_message(&SocketMessage::TogglePause)
                    //                .is_err()
                    //            {
                    //                tracing::error!(
                    //                    "could not send message to komorebi: TogglePause"
                    //                );
                    //            }
                    //        }
                    //        KomorebiLayout::Custom => {}
                    //    }
                    //}
                });
            }
        }

        if let Some(configuration_switcher) = &self.configuration_switcher {
            if configuration_switcher.enable {
                for (name, location) in configuration_switcher.configurations.iter() {
                    let path = PathBuf::from(location);
                    if path.is_file() {
                        config.apply_on_widget(true, ui,|ui|{
                    if ui
                            .add(Label::new(name).selectable(false).sense(Sense::click()))
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
                let titles = &komorebi_notification_state.focused_container_information.0;
                if !titles.is_empty() {
                    config.apply_on_widget(false, ui, |ui| {
                        let icons = &komorebi_notification_state.focused_container_information.1;
                        let focused_window_idx =
                            komorebi_notification_state.focused_container_information.2;

                        let iter = titles.iter().zip(icons.iter());

                        for (i, (title, icon)) in iter.enumerate() {
                            let selected = i == focused_window_idx;

                            if SelectableFrame::new(selected)
                                .show(ui, |ui| {
                                    if focused_window.show_icon {
                                        if let Some(img) = icon {
                                            ui.add(
                                                Image::from(&img_to_texture(ctx, img))
                                                    .maintain_aspect_ratio(true)
                                                    .max_height(15.0),
                                            );
                                        }
                                    }

                                    let available_height = ui.available_height();
                                    let mut custom_ui = CustomUi(ui);

                                    custom_ui.add_sized_left_to_right(
                                        Vec2::new(
                                            MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32,
                                            available_height,
                                        ),
                                        Label::new(title).selectable(false).truncate(),
                                    );
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
    pub workspaces: Vec<(String, bool)>,
    pub selected_workspace: String,
    pub focused_container_information: (Vec<String>, Vec<Option<RgbaImage>>, usize),
    pub layout: KomorebiLayout,
    pub hide_empty_workspaces: bool,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    pub stack_accent: Option<Color32>,
    pub monitor_index: usize,
}

#[derive(Copy, Clone, Debug)]
pub enum KomorebiLayout {
    Default(komorebi_client::DefaultLayout),
    Floating,
    Paused,
    Custom,
}

impl Display for KomorebiLayout {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KomorebiLayout::Default(layout) => write!(f, "{layout}"),
            KomorebiLayout::Floating => write!(f, "Floating"),
            KomorebiLayout::Paused => write!(f, "Paused"),
            KomorebiLayout::Custom => write!(f, "Custom"),
        }
    }
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

                    workspaces.push((
                        ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)),
                        should_show,
                    ));
                }

                self.workspaces = workspaces;
                self.layout = match monitor.workspaces()[focused_workspace_idx].layout() {
                    komorebi_client::Layout::Default(layout) => KomorebiLayout::Default(*layout),
                    komorebi_client::Layout::Custom(_) => KomorebiLayout::Custom,
                };

                if !*monitor.workspaces()[focused_workspace_idx].tile() {
                    self.layout = KomorebiLayout::Floating;
                }

                if notification.state.is_paused {
                    self.layout = KomorebiLayout::Paused;
                }

                let mut has_window_container_information = false;

                if let Some(container) =
                    monitor.workspaces()[focused_workspace_idx].monocle_container()
                {
                    has_window_container_information = true;
                    self.focused_container_information = (
                        container
                            .windows()
                            .iter()
                            .map(|w| w.title().unwrap_or_default())
                            .collect::<Vec<_>>(),
                        container
                            .windows()
                            .iter()
                            .map(|w| windows_icons::get_icon_by_process_id(w.process_id()))
                            .collect::<Vec<_>>(),
                        container.focused_window_idx(),
                    );
                } else if let Some(container) =
                    monitor.workspaces()[focused_workspace_idx].focused_container()
                {
                    has_window_container_information = true;
                    self.focused_container_information = (
                        container
                            .windows()
                            .iter()
                            .map(|w| w.title().unwrap_or_default())
                            .collect::<Vec<_>>(),
                        container
                            .windows()
                            .iter()
                            .map(|w| windows_icons::get_icon_by_process_id(w.process_id()))
                            .collect::<Vec<_>>(),
                        container.focused_window_idx(),
                    );
                }

                for floating_window in
                    monitor.workspaces()[focused_workspace_idx].floating_windows()
                {
                    if floating_window.is_focused() {
                        has_window_container_information = true;
                        self.focused_container_information = (
                            vec![floating_window.title().unwrap_or_default()],
                            vec![windows_icons::get_icon_by_process_id(
                                floating_window.process_id(),
                            )],
                            0,
                        );
                    }
                }

                if !has_window_container_information {
                    self.focused_container_information.0.clear();
                    self.focused_container_information.1.clear();
                    self.focused_container_information.2 = 0;
                }
            }
        }
    }
}
