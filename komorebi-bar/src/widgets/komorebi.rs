use super::ImageIcon;
use crate::bar::apply_theme;
use crate::config::DisplayFormat;
use crate::config::KomobarTheme;
use crate::config::WorkspacesDisplayFormat;
use crate::render::Grouping;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::ui::CustomUi;
use crate::widgets::komorebi_layout::KomorebiLayout;
use crate::widgets::widget::BarWidget;
use crate::MAX_LABEL_WIDTH;
use crate::MONITOR_INDEX;
use eframe::egui::text::LayoutJob;
use eframe::egui::vec2;
use eframe::egui::Align;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::CornerRadius;
use eframe::egui::Frame;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::Margin;
use eframe::egui::RichText;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::StrokeKind;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use komorebi_client::Container;
use komorebi_client::NotificationEvent;
use komorebi_client::PathExt;
use komorebi_client::Rect;
use komorebi_client::SocketMessage;
use komorebi_client::Window;
use komorebi_client::Workspace;
use komorebi_client::WorkspaceLayer;
use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KomorebiConfig {
    /// Configure the Workspaces widget
    pub workspaces: Option<KomorebiWorkspacesConfig>,
    /// Configure the Layout widget
    pub layout: Option<KomorebiLayoutConfig>,
    /// Configure the Workspace Layer widget
    pub workspace_layer: Option<KomorebiWorkspaceLayerConfig>,
    /// Configure the Focused Container widget
    #[serde(alias = "focused_window")]
    pub focused_container: Option<KomorebiFocusedContainerConfig>,
    /// Configure the Locked Container widget
    pub locked_container: Option<KomorebiLockedContainerConfig>,
    /// Configure the Configuration Switcher widget
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KomorebiWorkspacesConfig {
    /// Enable the Komorebi Workspaces widget
    pub enable: bool,
    /// Hide workspaces without any windows
    pub hide_empty_workspaces: bool,
    /// Display format of the workspace
    pub display: Option<WorkspacesDisplayFormat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KomorebiLayoutConfig {
    /// Enable the Komorebi Layout widget
    pub enable: bool,
    /// List of layout options
    pub options: Option<Vec<KomorebiLayout>>,
    /// Display format of the current layout
    pub display: Option<DisplayFormat>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KomorebiWorkspaceLayerConfig {
    /// Enable the Komorebi Workspace Layer widget
    pub enable: bool,
    /// Display format of the current layer
    pub display: Option<DisplayFormat>,
    /// Show the widget event if the layer is Tiling
    pub show_when_tiling: Option<bool>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KomorebiFocusedContainerConfig {
    /// Enable the Komorebi Focused Container widget
    pub enable: bool,
    /// DEPRECATED: use 'display' instead (Show the icon of the currently focused container)
    pub show_icon: Option<bool>,
    /// Display format of the currently focused container
    pub display: Option<DisplayFormat>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KomorebiLockedContainerConfig {
    /// Enable the Komorebi Locked Container widget
    pub enable: bool,
    /// Display format of the current locked state
    pub display: Option<DisplayFormat>,
    /// Show the widget event if the layer is unlocked
    pub show_when_unlocked: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
                    *location = dunce::simplified(&PathBuf::from(location.clone()).replace_env())
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
                hide_empty_workspaces: value
                    .workspaces
                    .map(|w| w.hide_empty_workspaces)
                    .unwrap_or_default(),
                mouse_follows_focus: true,
                work_area_offset: None,
                focused_container_information: (
                    false,
                    KomorebiNotificationStateContainerInformation::EMPTY,
                ),
                stack_accent: None,
                monitor_index: MONITOR_INDEX.load(Ordering::SeqCst),
                monitor_usr_idx_map: HashMap::new(),
            })),
            workspaces: value.workspaces,
            layout: value.layout.clone(),
            focused_container: value.focused_container,
            workspace_layer: value.workspace_layer,
            locked_container: value.locked_container,
            configuration_switcher,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Komorebi {
    pub komorebi_notification_state: Rc<RefCell<KomorebiNotificationState>>,
    pub workspaces: Option<KomorebiWorkspacesConfig>,
    pub layout: Option<KomorebiLayoutConfig>,
    pub focused_container: Option<KomorebiFocusedContainerConfig>,
    pub workspace_layer: Option<KomorebiWorkspaceLayerConfig>,
    pub locked_container: Option<KomorebiLockedContainerConfig>,
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

impl BarWidget for Komorebi {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let mut komorebi_notification_state = self.komorebi_notification_state.borrow_mut();
        let icon_size = Vec2::splat(config.icon_font_id.size);
        let text_size = Vec2::splat(config.text_font_id.size);

        if let Some(workspaces) = self.workspaces {
            if workspaces.enable {
                let mut update = None;

                if !komorebi_notification_state.workspaces.is_empty() {
                    let format = workspaces.display.unwrap_or(DisplayFormat::Text.into());

                    config.apply_on_widget(false, ui, |ui| {
                        for (i, (ws, containers, _, should_show)) in
                            komorebi_notification_state.workspaces.iter().enumerate()
                        {
                            if *should_show {
                            let is_selected = komorebi_notification_state.selected_workspace.eq(ws);

                            if SelectableFrame::new(
                                is_selected,
                            )
                            .show(ui, |ui| {
                                let mut has_icon = false;

                                if format == WorkspacesDisplayFormat::AllIcons
                                    || format == WorkspacesDisplayFormat::AllIconsAndText
                                    || format == WorkspacesDisplayFormat::AllIconsAndTextOnSelected
                                    || format == DisplayFormat::Icon.into()
                                    || format == DisplayFormat::IconAndText.into()
                                    || format == DisplayFormat::IconAndTextOnSelected.into()
                                    || (format == DisplayFormat::TextAndIconOnSelected.into() && is_selected)
                                {
                                    has_icon = containers.iter().any(|(_, container_info)| {
                                        container_info.icons.iter().any(|icon| icon.is_some())
                                    });

                                    if has_icon {
                                        Frame::NONE
                                            .inner_margin(Margin::same(
                                                ui.style().spacing.button_padding.y as i8,
                                            ))
                                            .show(ui, |ui| {
                                                for (is_focused, container) in containers {
                                                    for icon in container.icons.iter().flatten().collect::<Vec<_>>() {
                                                        ui.add(
                                                            Image::from(&icon.texture(ctx))
                                                                .maintain_aspect_ratio(true)
                                                                .fit_to_exact_size(if *is_focused { icon_size } else { text_size }),
                                                        );
                                                    }
                                                }
                                            });
                                    }
                                }

                                // draw a custom icon when there is no app icon or text
                                if !has_icon && (matches!(format, WorkspacesDisplayFormat::AllIcons | WorkspacesDisplayFormat::Existing(DisplayFormat::Icon))
                                || (!is_selected && matches!(format, WorkspacesDisplayFormat::AllIconsAndTextOnSelected | WorkspacesDisplayFormat::Existing(DisplayFormat::IconAndTextOnSelected)))) {
                                    let (response, painter) =
                                        ui.allocate_painter(icon_size, Sense::hover());
                                    let stroke = Stroke::new(
                                        1.0,
                                        if is_selected { ctx.style().visuals.selection.stroke.color} else { ui.style().visuals.text_color() },
                                    );
                                    let mut rect = response.rect;
                                    let rounding = CornerRadius::same((rect.width() * 0.1) as u8);
                                    rect = rect.shrink(stroke.width);
                                    let c = rect.center();
                                    let r = rect.width() / 2.0;
                                    painter.rect_stroke(rect, rounding, stroke, StrokeKind::Outside);
                                    painter.line_segment([c - vec2(r, r), c + vec2(r, r)], stroke);

                                    response.on_hover_text(ws.to_string())
                                // add hover text when there are only icons
                                } else if match format {
                                    WorkspacesDisplayFormat::AllIcons | WorkspacesDisplayFormat::Existing(DisplayFormat::Icon) => has_icon,
                                    _ => false,
                                } {
                                    ui.response().on_hover_text(ws.to_string())
                                // add label only
                                } else if (format != WorkspacesDisplayFormat::AllIconsAndTextOnSelected && format != DisplayFormat::IconAndTextOnSelected.into())
                                    || (is_selected && matches!(format, WorkspacesDisplayFormat::AllIconsAndTextOnSelected | WorkspacesDisplayFormat::Existing(DisplayFormat::IconAndTextOnSelected)))
                                {
                                     if is_selected {
                                        ui.add(Label::new(RichText::new(ws.to_string()).color(ctx.style().visuals.selection.stroke.color)).selectable(false))
                                    }
                                    else {
                                        ui.add(Label::new(ws.to_string()).selectable(false))
                                    }
                                } else {
                                    ui.response()
                                }
                            })
                            .clicked()
                            {
                                update = Some(ws.to_string());

                                if komorebi_notification_state.mouse_follows_focus {
                                    if komorebi_client::send_batch([
                                        SocketMessage::MouseFollowsFocus(false),
                                        SocketMessage::FocusMonitorWorkspaceNumber(
                                            komorebi_notification_state.monitor_index,
                                            i,
                                        ),
                                        SocketMessage::MouseFollowsFocus(true),
                                    ])
                                        .is_err()
                                    {
                                        tracing::error!(
                                            "could not send the following batch of messages to komorebi:\n
                                            MouseFollowsFocus(false)\n
                                            FocusMonitorWorkspaceNumber({}, {})\n
                                            MouseFollowsFocus(true)\n",
                                            komorebi_notification_state.monitor_index,
                                            i,
                                        );
                                    }
                                } else if komorebi_client::send_batch([
                                    SocketMessage::FocusMonitorWorkspaceNumber(
                                        komorebi_notification_state.monitor_index,
                                        i,
                                    ),
                                ])
                                    .is_err()
                                {
                                    tracing::error!(
                                        "could not send the following batch of messages to komorebi:\n
                                        FocusMonitorWorkspaceNumber({}, {})\n",
                                        komorebi_notification_state.monitor_index,
                                        i,
                                    );
                                }
                            }
                            }
                        }
                    });
                }

                if let Some(update) = update {
                    komorebi_notification_state.selected_workspace = update;
                }
            }
        }

        if let Some(layer_config) = &self.workspace_layer {
            if layer_config.enable {
                let layer = komorebi_notification_state
                    .workspaces
                    .iter()
                    .find(|o| komorebi_notification_state.selected_workspace.eq(&o.0))
                    .map(|(_, _, layer, _)| layer);

                if let Some(layer) = layer {
                    if (layer_config.show_when_tiling.unwrap_or_default()
                        && matches!(layer, WorkspaceLayer::Tiling))
                        || matches!(layer, WorkspaceLayer::Floating)
                    {
                        let display_format = layer_config.display.unwrap_or(DisplayFormat::Text);
                        let size = Vec2::splat(config.icon_font_id.size);

                        config.apply_on_widget(false, ui, |ui| {
                            let layer_frame = SelectableFrame::new(false)
                                .show(ui, |ui| {
                                    if display_format != DisplayFormat::Text {
                                        if matches!(layer, WorkspaceLayer::Tiling) {
                                            let (response, painter) =
                                                ui.allocate_painter(size, Sense::hover());
                                            let color = ctx.style().visuals.selection.stroke.color;
                                            let stroke = Stroke::new(1.0, color);
                                            let mut rect = response.rect;
                                            let corner =
                                                CornerRadius::same((rect.width() * 0.1) as u8);
                                            rect = rect.shrink(stroke.width);

                                            // tiling
                                            let mut rect_left = response.rect;
                                            rect_left.set_width(rect.width() * 0.48);
                                            rect_left.set_height(rect.height() * 0.98);
                                            let mut rect_right = rect_left;
                                            rect_left = rect_left.translate(Vec2::new(
                                                rect.width() * 0.01 + stroke.width,
                                                rect.width() * 0.01 + stroke.width,
                                            ));
                                            rect_right = rect_right.translate(Vec2::new(
                                                rect.width() * 0.51 + stroke.width,
                                                rect.width() * 0.01 + stroke.width,
                                            ));
                                            painter.rect_filled(rect_left, corner, color);
                                            painter.rect_stroke(
                                                rect_right,
                                                corner,
                                                stroke,
                                                StrokeKind::Outside,
                                            );
                                        } else {
                                            let (response, painter) =
                                                ui.allocate_painter(size, Sense::hover());
                                            let color = ctx.style().visuals.selection.stroke.color;
                                            let stroke = Stroke::new(1.0, color);
                                            let mut rect = response.rect;
                                            let corner =
                                                CornerRadius::same((rect.width() * 0.1) as u8);
                                            rect = rect.shrink(stroke.width);

                                            // floating
                                            let mut rect_left = response.rect;
                                            rect_left.set_width(rect.width() * 0.65);
                                            rect_left.set_height(rect.height() * 0.65);
                                            let mut rect_right = rect_left;
                                            rect_left = rect_left.translate(Vec2::new(
                                                rect.width() * 0.01 + stroke.width,
                                                rect.width() * 0.01 + stroke.width,
                                            ));
                                            rect_right = rect_right.translate(Vec2::new(
                                                rect.width() * 0.34 + stroke.width,
                                                rect.width() * 0.34 + stroke.width,
                                            ));
                                            painter.rect_filled(rect_left, corner, color);
                                            painter.rect_stroke(
                                                rect_right,
                                                corner,
                                                stroke,
                                                StrokeKind::Outside,
                                            );
                                        }
                                    }

                                    if display_format != DisplayFormat::Icon {
                                        ui.add(Label::new(layer.to_string()).selectable(false));
                                    }
                                })
                                .on_hover_text(layer.to_string());

                            if layer_frame.clicked()
                                && komorebi_client::send_batch([
                                    SocketMessage::FocusMonitorAtCursor,
                                    SocketMessage::MouseFollowsFocus(false),
                                    SocketMessage::ToggleWorkspaceLayer,
                                    SocketMessage::MouseFollowsFocus(
                                        komorebi_notification_state.mouse_follows_focus,
                                    ),
                                ])
                                .is_err()
                            {
                                tracing::error!(
                                    "could not send the following batch of messages to komorebi:\n\
                                                MouseFollowsFocus(false),
                                                ToggleWorkspaceLayer,
                                                MouseFollowsFocus({})",
                                    komorebi_notification_state.mouse_follows_focus,
                                );
                            }
                        });
                    }
                }
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
                        config.apply_on_widget(false, ui, |ui| {
                            if SelectableFrame::new(false)
                                .show(ui, |ui| ui.add(Label::new(name).selectable(false)))
                                .clicked()
                            {
                                let canonicalized =
                                    dunce::canonicalize(path.clone()).unwrap_or(path);

                                if komorebi_client::send_message(
                                    &SocketMessage::ReplaceConfiguration(canonicalized),
                                )
                                .is_err()
                                {
                                    tracing::error!(
                                        "could not send message to komorebi: ReplaceConfiguration"
                                    );
                                }
                            }
                        });
                    }
                }
            }
        }

        if let Some(locked_container_config) = self.locked_container {
            if locked_container_config.enable {
                let is_locked = komorebi_notification_state.focused_container_information.0;

                if locked_container_config
                    .show_when_unlocked
                    .unwrap_or_default()
                    || is_locked
                {
                    let titles = &komorebi_notification_state
                        .focused_container_information
                        .1
                        .titles;

                    if !titles.is_empty() {
                        let display_format = locked_container_config
                            .display
                            .unwrap_or(DisplayFormat::Text);

                        let mut layout_job = LayoutJob::simple(
                            if display_format != DisplayFormat::Text {
                                if is_locked {
                                    egui_phosphor::regular::LOCK_KEY.to_string()
                                } else {
                                    egui_phosphor::regular::LOCK_SIMPLE_OPEN.to_string()
                                }
                            } else {
                                String::new()
                            },
                            config.icon_font_id.clone(),
                            ctx.style().visuals.selection.stroke.color,
                            100.0,
                        );

                        if display_format != DisplayFormat::Icon {
                            layout_job.append(
                                if is_locked { "Locked" } else { "Unlocked" },
                                10.0,
                                TextFormat {
                                    font_id: config.text_font_id.clone(),
                                    color: ctx.style().visuals.text_color(),
                                    valign: Align::Center,
                                    ..Default::default()
                                },
                            );
                        }

                        config.apply_on_widget(false, ui, |ui| {
                            if SelectableFrame::new(false)
                                .show(ui, |ui| ui.add(Label::new(layout_job).selectable(false)))
                                .clicked()
                                && komorebi_client::send_batch([
                                    SocketMessage::FocusMonitorAtCursor,
                                    SocketMessage::ToggleLock,
                                ])
                                .is_err()
                            {
                                tracing::error!("could not send ToggleLock");
                            }
                        });
                    }
                }
            }
        }

        if let Some(focused_container_config) = self.focused_container {
            if focused_container_config.enable {
                let titles = &komorebi_notification_state
                    .focused_container_information
                    .1
                    .titles;

                if !titles.is_empty() {
                    config.apply_on_widget(false, ui, |ui| {
                        let icons = &komorebi_notification_state
                            .focused_container_information.1
                            .icons;
                        let focused_window_idx = komorebi_notification_state
                            .focused_container_information.1
                            .focused_window_idx;

                        let iter = titles.iter().zip(icons.iter());
                        let len = iter.len();

                        for (i, (title, icon)) in iter.enumerate() {
                            let selected = i == focused_window_idx && len != 1;
                            let text_color = if selected { ctx.style().visuals.selection.stroke.color } else { ui.style().visuals.text_color() };

                            if SelectableFrame::new(selected)
                                .show(ui, |ui| {
                                    // handle legacy setting
                                    let format = focused_container_config.display.unwrap_or(
                                        if focused_container_config.show_icon.unwrap_or(false) {
                                            DisplayFormat::IconAndText
                                        } else {
                                            DisplayFormat::Text
                                        },
                                    );

                                    if format == DisplayFormat::Icon
                                        || format == DisplayFormat::IconAndText
                                        || format == DisplayFormat::IconAndTextOnSelected
                                        || (format == DisplayFormat::TextAndIconOnSelected
                                            && i == focused_window_idx)
                                    {
                                        if let Some(img) = icon {
                                            Frame::NONE
                                                .inner_margin(Margin::same(
                                                    ui.style().spacing.button_padding.y as i8,
                                                ))
                                                .show(ui, |ui| {
                                                    let response = ui.add(
                                                        Image::from(&img.texture(ctx) )
                                                            .maintain_aspect_ratio(true)
                                                            .fit_to_exact_size(icon_size),
                                                    );

                                                    if let DisplayFormat::Icon = format {
                                                        response.on_hover_text(title);
                                                    }
                                                });
                                        }
                                    }

                                    if format == DisplayFormat::Text
                                        || format == DisplayFormat::IconAndText
                                        || format == DisplayFormat::TextAndIconOnSelected
                                        || (format == DisplayFormat::IconAndTextOnSelected
                                            && i == focused_window_idx)
                                    {
                                        let available_height = ui.available_height();
                                        let mut custom_ui = CustomUi(ui);

                                        custom_ui.add_sized_left_to_right(
                                            Vec2::new(
                                                MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32,
                                                available_height,
                                            ),
                                            Label::new(RichText::new( title).color(text_color)).selectable(false).truncate(),
                                        );
                                    }
                                })
                                .clicked()
                            {
                                if selected {
                                    return;
                                }

                                if komorebi_notification_state.mouse_follows_focus {
                                    if komorebi_client::send_batch([
                                        SocketMessage::MouseFollowsFocus(false),
                                        SocketMessage::FocusStackWindow(i),
                                        SocketMessage::MouseFollowsFocus(true),
                                    ]).is_err() {
                                        tracing::error!(
                                            "could not send the following batch of messages to komorebi:\n
                                            MouseFollowsFocus(false)\n
                                            FocusStackWindow({})\n
                                            MouseFollowsFocus(true)\n",
                                            i,
                                        );
                                    }
                                } else if komorebi_client::send_message(
                                    &SocketMessage::FocusStackWindow(i)
                                ).is_err() {
                                    tracing::error!(
                                        "could not send message to komorebi: FocusStackWindow"
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

#[allow(clippy::type_complexity)]
#[derive(Clone, Debug)]
pub struct KomorebiNotificationState {
    pub workspaces: Vec<(
        String,
        Vec<(bool, KomorebiNotificationStateContainerInformation)>,
        WorkspaceLayer,
        bool,
    )>,
    pub selected_workspace: String,
    pub focused_container_information: (bool, KomorebiNotificationStateContainerInformation),
    pub layout: KomorebiLayout,
    pub hide_empty_workspaces: bool,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    pub stack_accent: Option<Color32>,
    pub monitor_index: usize,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
}

impl KomorebiNotificationState {
    pub fn update_from_config(&mut self, config: &Self) {
        self.hide_empty_workspaces = config.hide_empty_workspaces;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_notification(
        &mut self,
        ctx: &Context,
        monitor_index: Option<usize>,
        notification: komorebi_client::Notification,
        bg_color: Rc<RefCell<Color32>>,
        bg_color_with_alpha: Rc<RefCell<Color32>>,
        transparency_alpha: Option<u8>,
        grouping: Option<Grouping>,
        default_theme: Option<KomobarTheme>,
        render_config: Rc<RefCell<RenderConfig>>,
    ) {
        let show_all_icons = render_config.borrow().show_all_icons;

        match notification.event {
            NotificationEvent::VirtualDesktop(_) => {}
            NotificationEvent::WindowManager(_) => {}
            NotificationEvent::Monitor(_) => {}
            NotificationEvent::Socket(message) => match message {
                SocketMessage::ReloadStaticConfiguration(path) => {
                    if let Ok(config) = komorebi_client::StaticConfig::read(&path) {
                        if let Some(theme) = config.theme {
                            apply_theme(
                                ctx,
                                KomobarTheme::from(theme),
                                bg_color.clone(),
                                bg_color_with_alpha.clone(),
                                transparency_alpha,
                                grouping,
                                render_config,
                            );
                            tracing::info!("applied theme from updated komorebi.json");
                        } else if let Some(default_theme) = default_theme {
                            apply_theme(
                                ctx,
                                default_theme,
                                bg_color.clone(),
                                bg_color_with_alpha.clone(),
                                transparency_alpha,
                                grouping,
                                render_config,
                            );
                            tracing::info!("removed theme from updated komorebi.json and applied default theme");
                        } else {
                            tracing::warn!("theme was removed from updated komorebi.json but there was no default theme to apply");
                        }
                    }
                }
                SocketMessage::Theme(theme) => {
                    apply_theme(
                        ctx,
                        KomobarTheme::from(*theme),
                        bg_color,
                        bg_color_with_alpha.clone(),
                        transparency_alpha,
                        grouping,
                        render_config,
                    );
                    tracing::info!("applied theme from komorebi socket message");
                }
                _ => {}
            },
        }

        self.monitor_usr_idx_map = notification.state.monitor_usr_idx_map.clone();

        if monitor_index.is_none()
            || monitor_index.is_some_and(|idx| idx >= notification.state.monitors.elements().len())
        {
            // The bar's monitor is diconnected, so the bar is disabled no need to check anything
            // any further otherwise we'll get `OutOfBounds` panics.
            return;
        }
        let monitor_index = monitor_index.expect("should have a monitor index");
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
                focused_workspace_idx == i || !ws.is_empty()
            } else {
                true
            };

            workspaces.push((
                ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)),
                if show_all_icons {
                    let mut containers = vec![];
                    let mut has_monocle = false;

                    // add monocle container
                    if let Some(container) = ws.monocle_container() {
                        containers.push((true, container.into()));
                        has_monocle = true;
                    }

                    // add all tiled windows
                    for (i, container) in ws.containers().iter().enumerate() {
                        containers.push((
                            !has_monocle && i == ws.focused_container_idx(),
                            container.into(),
                        ));
                    }

                    // add all floating windows
                    for floating_window in ws.floating_windows() {
                        containers.push((
                            !has_monocle && floating_window.is_focused(),
                            floating_window.into(),
                        ));
                    }

                    containers
                } else {
                    vec![(true, ws.into())]
                },
                ws.layer().to_owned(),
                should_show,
            ));
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
                komorebi_client::Layout::Default(layout) => KomorebiLayout::Default(*layout),
                komorebi_client::Layout::Custom(_) => KomorebiLayout::Custom,
            };
        }

        let focused_workspace = &monitor.workspaces()[focused_workspace_idx];
        let is_locked = match focused_workspace.focused_container() {
            Some(container) => container.locked(),
            None => false,
        };

        self.focused_container_information = (is_locked, focused_workspace.into());
    }
}

#[derive(Clone, Debug)]
pub struct KomorebiNotificationStateContainerInformation {
    pub titles: Vec<String>,
    pub icons: Vec<Option<ImageIcon>>,
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
        let windows = value.windows().iter().collect::<Vec<_>>();

        let icons = windows
            .iter()
            .map(|window| {
                ImageIcon::try_load(window.hwnd, || {
                    windows_icons::get_icon_by_hwnd(window.hwnd).or_else(|| {
                        windows_icons_fallback::get_icon_by_process_id(window.process_id())
                    })
                })
            })
            .collect::<Vec<_>>();

        Self {
            titles: value
                .windows()
                .iter()
                .map(|w| w.title().unwrap_or_default())
                .collect::<Vec<_>>(),
            icons,
            focused_window_idx: value.focused_window_idx(),
        }
    }
}

impl From<&Window> for KomorebiNotificationStateContainerInformation {
    fn from(value: &Window) -> Self {
        let icons = ImageIcon::try_load(value.hwnd, || {
            windows_icons::get_icon_by_hwnd(value.hwnd)
                .or_else(|| windows_icons_fallback::get_icon_by_process_id(value.process_id()))
        });

        Self {
            titles: vec![value.title().unwrap_or_default()],
            icons: vec![icons],
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
