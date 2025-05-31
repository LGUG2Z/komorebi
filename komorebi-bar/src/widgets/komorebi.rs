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
use eframe::egui::Response;
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
use komorebi_client::State;
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
            komorebi_notification_state: Rc::new(RefCell::new(KomorebiNotificationStateNew(
                MonitorInfo {
                    workspaces: Vec::new(),
                    layout: KomorebiLayout::Default(komorebi_client::DefaultLayout::BSP),
                    mouse_follows_focus: true,
                    work_area_offset: None,
                    stack_accent: None,
                    monitor_index: MONITOR_INDEX.load(Ordering::SeqCst),
                    monitor_usr_idx_map: HashMap::new(),
                    focused_workspace_idx: None,
                    show_all_icons: false,
                    hide_empty_workspaces: value
                        .workspaces
                        .map(|w| w.hide_empty_workspaces)
                        .unwrap_or_default(),
                },
            ))),
            workspaces: value.workspaces.map(WorkspaceWidget::from),
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
    pub komorebi_notification_state: Rc<RefCell<KomorebiNotificationStateNew>>,
    pub workspaces: Option<WorkspaceWidget>,
    pub layout: Option<KomorebiLayoutConfig>,
    pub focused_container: Option<KomorebiFocusedContainerConfig>,
    pub workspace_layer: Option<KomorebiWorkspaceLayerConfig>,
    pub locked_container: Option<KomorebiLockedContainerConfig>,
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

impl BarWidget for Komorebi {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let icon_size = Vec2::splat(config.icon_font_id.size);
        let text_size = Vec2::splat(config.text_font_id.size);

        self.render_workspaces_widget(ctx, ui, config, icon_size, text_size);

        let monitor_info = &mut self.komorebi_notification_state.borrow_mut().0;

        if let Some(layer_config) = &self.workspace_layer {
            if layer_config.enable {
                if let Some(layer) = monitor_info.focused_workspace_layer() {
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
                                        monitor_info.mouse_follows_focus,
                                    ),
                                ])
                                .is_err()
                            {
                                tracing::error!(
                                    "could not send the following batch of messages to komorebi:\n\
                                                MouseFollowsFocus(false),
                                                ToggleWorkspaceLayer,
                                                MouseFollowsFocus({})",
                                    monitor_info.mouse_follows_focus,
                                );
                            }
                        });
                    }
                }
            }
        }

        if let Some(layout_config) = &self.layout {
            if layout_config.enable {
                let workspace_idx = monitor_info.focused_workspace_idx;
                monitor_info
                    .layout
                    .show(ctx, ui, config, layout_config, workspace_idx);
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
                let focused_container = monitor_info.focused_container();
                let is_locked = focused_container
                    .map(|container| container.is_locked)
                    .unwrap_or_default();

                if locked_container_config
                    .show_when_unlocked
                    .unwrap_or_default()
                    || is_locked
                {
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

        if let Some(focused_container_config) = self.focused_container {
            if focused_container_config.enable {
                if let Some(container) = monitor_info.focused_container() {
                    config.apply_on_widget(false, ui, |ui| {
                        let focused_window_idx = container.focused_window_idx;
                        let len = container.windows.len();

                        for (this_idx, window) in container.windows.iter().enumerate() {
                            let selected = this_idx == focused_window_idx && len != 1;
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
                                            && this_idx == focused_window_idx)
                                    {
                                        if let Some(img) = &window.icon {
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

                                                    match (format, &window.title) {
                                                        (DisplayFormat::Icon, Some(title)) => response.on_hover_text(title),
                                                        _ => response,
                                                    }
                                                });
                                        }
                                    }

                                    if format == DisplayFormat::Text
                                        || format == DisplayFormat::IconAndText
                                        || format == DisplayFormat::TextAndIconOnSelected
                                        || (format == DisplayFormat::IconAndTextOnSelected
                                            && this_idx == focused_window_idx)
                                    {
                                        if let Some(title) = &window.title {
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
                                    }
                                })
                                .clicked()
                            {
                                if selected {
                                    return;
                                }

                                if monitor_info.mouse_follows_focus {
                                    if komorebi_client::send_batch([
                                        SocketMessage::MouseFollowsFocus(false),
                                        SocketMessage::FocusStackWindow(this_idx),
                                        SocketMessage::MouseFollowsFocus(true),
                                    ]).is_err() {
                                        tracing::error!(
                                            "could not send the following batch of messages to komorebi:\n
                                            MouseFollowsFocus(false)\n
                                            FocusStackWindow({})\n
                                            MouseFollowsFocus(true)\n",
                                            this_idx,
                                        );
                                    }
                                } else if komorebi_client::send_message(
                                    &SocketMessage::FocusStackWindow(this_idx)
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

impl Komorebi {
    fn render_workspaces_widget(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        config: &mut RenderConfig,
        icon_size: Vec2,
        text_size: Vec2,
    ) {
        let monitor_info = &mut self.komorebi_notification_state.borrow_mut().0;

        let workspace_widget = match &self.workspaces {
            Some(wg) if wg.enable && !monitor_info.workspaces.is_empty() => wg,
            _ => return,
        };

        config.apply_on_widget(false, ui, |ui| {
            for (index, workspace) in monitor_info.workspaces.iter().enumerate() {
                if !workspace.should_show {
                    continue;
                }

                if (workspace_widget.renderer)(ctx, ui, workspace, icon_size, text_size).clicked() {
                    Self::focus_workspace(
                        monitor_info.monitor_index,
                        index,
                        monitor_info.mouse_follows_focus,
                    );
                    monitor_info.focused_workspace_idx = Some(index);
                }
            }
        });
    }

    fn focus_workspace(monitor_index: usize, workspace_index: usize, mouse_follows_focus: bool) {
        if mouse_follows_focus {
            let messages = [
                SocketMessage::MouseFollowsFocus(false),
                SocketMessage::FocusMonitorWorkspaceNumber(monitor_index, workspace_index),
                SocketMessage::MouseFollowsFocus(true),
            ];
            if komorebi_client::send_batch(messages).is_err() {
                tracing::error!(
                    "could not send the following batch of messages to komorebi:\n\
                    MouseFollowsFocus(false)\n\
                    FocusMonitorWorkspaceNumber({}, {})\n\
                    MouseFollowsFocus(true)\n",
                    monitor_index,
                    workspace_index,
                );
            }
        } else {
            let messages = [SocketMessage::FocusMonitorWorkspaceNumber(
                monitor_index,
                workspace_index,
            )];
            if komorebi_client::send_batch(messages).is_err() {
                tracing::error!(
                    "could not send the following batch of messages to komorebi:\n\
                     FocusMonitorWorkspaceNumber({}, {})\n",
                    monitor_index,
                    workspace_index,
                );
            }
        }
    }
}

/// WorkspaceWidget with pre-selected render strategy for workspace display.
///
/// The `renderer` field points to the correct rendering function
/// based on the configured `WorkspacesDisplayFormat`.
#[derive(Clone, Debug)]
pub struct WorkspaceWidget {
    /// Chosen rendering function for this widget
    renderer: fn(&Context, &mut Ui, &WorkspaceInfo, Vec2, Vec2) -> Response,
    /// Whether the widget is enabled
    pub enable: bool,
}

impl From<KomorebiWorkspacesConfig> for WorkspaceWidget {
    fn from(value: KomorebiWorkspacesConfig) -> Self {
        use WorkspacesDisplayFormat::*;
        // Selects a render strategy according to the workspace config's display format
        // for better performance
        let renderer = match value.display.unwrap_or(DisplayFormat::Text.into()) {
            // Case 1: - Show icons if any, fallback if none
            //         - Only hover workspace name
            AllIcons | Existing(DisplayFormat::Icon) => Self::render_all_icons,
            // Case 2: - Show icons if any, with no fallback
            //         - Label workspace name with color if selected (no hover)
            AllIconsAndText | Existing(DisplayFormat::IconAndText) => {
                Self::render_all_icons_and_text
            }
            // Case 3: - Show icons if any, fallback only if not selected and no icons
            //         - Label workspace name only if selected (always hover name)
            AllIconsAndTextOnSelected | Existing(DisplayFormat::IconAndTextOnSelected) => {
                Self::render_all_icons_and_text_on_selected
            }
            // Case 4: - Show icons if selected and has icons (no fallback icon)
            //         - Label workspace name (with color if selected)
            Existing(DisplayFormat::TextAndIconOnSelected) => {
                Self::render_text_and_icon_on_selected
            }
            // Case 5: - Never show icon (no icons at all)
            //         - Label workspace name always (with color if selected)
            Existing(DisplayFormat::Text) => Self::render_text,
        };

        Self {
            renderer,
            enable: value.enable,
        }
    }
}

impl WorkspaceWidget {
    /// Renders workspace: icons if present, otherwise fallback icon.
    /// Displays only the workspace name as hover tooltip (no visible label).
    fn render_all_icons(
        ctx: &Context,
        ui: &mut Ui,
        ws: &WorkspaceInfo,
        icon_size: Vec2,
        text_size: Vec2,
    ) -> Response {
        if !Self::render_icons_no_fallback(ctx, ui, ws, icon_size, text_size) {
            Self::render_fallback_icon(ctx, ui, ws, icon_size);
        }
        ui.response().on_hover_text(&ws.name)
    }

    /// Renders workspace: icons if present (no fallback).
    /// Always displays the workspace label (highlighted if selected).
    fn render_all_icons_and_text(
        ctx: &Context,
        ui: &mut Ui,
        ws: &WorkspaceInfo,
        icon_size: Vec2,
        text_size: Vec2,
    ) -> Response {
        Self::render_icons_no_fallback(ctx, ui, ws, icon_size, text_size);
        Self::render_label(ctx, ui, ws)
    }

    /// Renders workspace: icons if present, fallback icon only if not selected
    /// and no icons. Displays the workspace label only if selected. Always shows
    /// workspace name as hover tooltip.
    fn render_all_icons_and_text_on_selected(
        ctx: &Context,
        ui: &mut Ui,
        ws: &WorkspaceInfo,
        icon_size: Vec2,
        text_size: Vec2,
    ) -> Response {
        let has_icon = Self::render_icons_no_fallback(ctx, ui, ws, icon_size, text_size);
        if !has_icon && !ws.is_selected {
            Self::render_fallback_icon(ctx, ui, ws, icon_size);
        }

        let resp = if ws.is_selected {
            Self::render_label(ctx, ui, ws)
        } else {
            ui.response()
        };
        resp.on_hover_text(&ws.name)
    }

    /// Renders workspace: icons only if selected. Always displays the workspace label
    /// (highlighted if selected).
    fn render_text_and_icon_on_selected(
        ctx: &Context,
        ui: &mut Ui,
        ws: &WorkspaceInfo,
        icon_size: Vec2,
        text_size: Vec2,
    ) -> Response {
        if ws.is_selected {
            Self::render_icons_no_fallback(ctx, ui, ws, icon_size, text_size);
        }
        Self::render_label(ctx, ui, ws)
    }

    /// Renders workspace: never displays icons. Always displays the workspace label
    /// (highlighted if selected).
    fn render_text(ctx: &Context, ui: &mut Ui, ws: &WorkspaceInfo, _: Vec2, _: Vec2) -> Response {
        Self::render_label(ctx, ui, ws)
    }

    /// Draws application icons for a workspace, if present.
    /// Returns true if any icon was drawn, false otherwise.
    fn render_icons_no_fallback(
        ctx: &Context,
        ui: &mut Ui,
        ws: &WorkspaceInfo,
        icon_size: Vec2,
        text_size: Vec2,
    ) -> bool {
        let has_icon = ws
            .containers
            .iter()
            .any(|container| container.windows.iter().any(|window| window.icon.is_some()));

        if has_icon {
            Frame::NONE
                .inner_margin(Margin::same(ui.style().spacing.button_padding.y as i8))
                .show(ui, |ui| {
                    for container in &ws.containers {
                        for icon in container.windows.iter().filter_map(|win| win.icon.as_ref()) {
                            ui.add(
                                Image::from(&icon.texture(ctx))
                                    .maintain_aspect_ratio(true)
                                    .fit_to_exact_size(if container.is_focused {
                                        icon_size
                                    } else {
                                        text_size
                                    }),
                            );
                        }
                    }
                });
        }
        has_icon
    }

    /// Draws a fallback icon (a rectangle with a diagonal) for the workspace.
    fn render_fallback_icon(
        ctx: &Context,
        ui: &mut Ui,
        ws: &WorkspaceInfo,
        icon_size: Vec2,
    ) -> Response {
        let (response, painter) = ui.allocate_painter(icon_size, Sense::hover());
        let stroke = Stroke::new(
            1.0,
            if ws.is_selected {
                ctx.style().visuals.selection.stroke.color
            } else {
                ui.style().visuals.text_color()
            },
        );
        let mut rect = response.rect;
        let rounding = CornerRadius::same((rect.width() * 0.1) as u8);
        rect = rect.shrink(stroke.width);
        let center = rect.center();
        let radius = rect.width() / 2.0;
        painter.rect_stroke(rect, rounding, stroke, StrokeKind::Outside);
        let points = [center - vec2(radius, radius), center + vec2(radius, radius)];
        painter.line_segment(points, stroke);
        response
    }

    /// Renders the workspace label (colored if selected).
    fn render_label(ctx: &Context, ui: &mut Ui, ws: &WorkspaceInfo) -> Response {
        if ws.is_selected {
            let text = RichText::new(&ws.name).color(ctx.style().visuals.selection.stroke.color);
            ui.add(Label::new(text).selectable(false))
        } else {
            ui.add(Label::new(&ws.name).selectable(false))
        }
    }
}

#[derive(Clone, Debug)]
#[repr(transparent)]
// TODO: Remove this wrapper
pub struct KomorebiNotificationStateNew(pub MonitorInfo);

impl KomorebiNotificationStateNew {
    pub fn update_from_config(&mut self, config: &Self) {
        self.0.hide_empty_workspaces = config.0.hide_empty_workspaces;
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
        self.0.update(
            monitor_index,
            notification.state,
            render_config.borrow().show_all_icons,
        );

        if let NotificationEvent::Socket(message) = notification.event {
            match message {
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
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub workspaces: Vec<WorkspaceInfo>,
    pub layout: KomorebiLayout,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    pub stack_accent: Option<Color32>,
    pub monitor_index: usize,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    pub focused_workspace_idx: Option<usize>,
    pub show_all_icons: bool,
    pub hide_empty_workspaces: bool,
}

impl MonitorInfo {
    pub fn focused_workspace(&self) -> Option<&WorkspaceInfo> {
        self.workspaces.get(self.focused_workspace_idx?)
    }

    pub fn focused_workspace_layer(&self) -> Option<WorkspaceLayer> {
        self.focused_workspace().map(|ws| ws.layer)
    }

    pub fn focused_container(&self) -> Option<&ContainerInfo> {
        self.focused_workspace()
            .and_then(WorkspaceInfo::focused_container)
    }
}

impl MonitorInfo {
    pub fn update(&mut self, monitor_index: Option<usize>, state: State, show_all_icons: bool) {
        self.show_all_icons = show_all_icons;
        self.monitor_usr_idx_map = state.monitor_usr_idx_map;

        match monitor_index {
            Some(idx) if idx < state.monitors.elements().len() => self.monitor_index = idx,
            // The bar's monitor is diconnected, so the bar is disabled no need to check anything
            // any further otherwise we'll get `OutOfBounds` panics.
            _ => return,
        };
        self.mouse_follows_focus = state.mouse_follows_focus;

        let monitor = &state.monitors.elements()[self.monitor_index];
        self.work_area_offset = monitor.work_area_offset();
        self.focused_workspace_idx = Some(monitor.focused_workspace_idx());

        // Layout
        let focused_ws = &monitor.workspaces()[monitor.focused_workspace_idx()];
        self.layout = Self::resolve_layout(focused_ws, state.is_paused);

        self.workspaces.clear();
        self.workspaces.extend(Self::workspaces(
            self.show_all_icons,
            self.hide_empty_workspaces,
            self.focused_workspace_idx,
            monitor.workspaces().iter().enumerate(),
        ));
    }

    fn workspaces<'a, I>(
        show_all_icons: bool,
        hide_empty_ws: bool,
        focused_ws_idx: Option<usize>,
        iter: I,
    ) -> impl Iterator<Item = WorkspaceInfo> + 'a
    where
        I: Iterator<Item = (usize, &'a Workspace)> + 'a,
    {
        let fn_containers_from = if show_all_icons {
            |ws| ContainerInfo::from_all_containers(ws)
        } else {
            |ws| {
                ContainerInfo::from_focused_container(ws)
                    .into_iter()
                    .collect()
            }
        };
        iter.map(move |(index, ws)| {
            let containers = fn_containers_from(ws);
            WorkspaceInfo {
                name: ws
                    .name()
                    .to_owned()
                    .unwrap_or_else(|| format!("{}", index + 1)),
                focused_container_idx: containers.iter().position(|c| c.is_focused),
                containers,
                layer: *ws.layer(),
                should_show: !hide_empty_ws || focused_ws_idx == Some(index) || !ws.is_empty(),
                is_selected: focused_ws_idx == Some(index),
            }
        })
    }

    fn resolve_layout(focused_ws: &Workspace, is_paused: bool) -> KomorebiLayout {
        if focused_ws.monocle_container().is_some() {
            KomorebiLayout::Monocle
        } else if !focused_ws.tile() {
            KomorebiLayout::Floating
        } else if is_paused {
            KomorebiLayout::Paused
        } else {
            match focused_ws.layout() {
                komorebi_client::Layout::Default(layout) => KomorebiLayout::Default(*layout),
                komorebi_client::Layout::Custom(_) => KomorebiLayout::Custom,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceInfo {
    pub name: String,
    pub containers: Vec<ContainerInfo>,
    pub focused_container_idx: Option<usize>,
    pub layer: WorkspaceLayer,
    pub should_show: bool,
    pub is_selected: bool,
}

impl WorkspaceInfo {
    pub fn focused_container(&self) -> Option<&ContainerInfo> {
        self.containers.get(self.focused_container_idx?)
    }
}

#[derive(Clone, Debug)]
pub struct ContainerInfo {
    pub windows: Vec<WindowInfo>,
    pub focused_window_idx: usize,
    pub is_focused: bool,
    pub is_locked: bool,
}

impl ContainerInfo {
    pub fn from_all_containers(ws: &Workspace) -> Vec<Self> {
        let has_focused_float = ws.floating_windows().iter().any(|w| w.is_focused());

        // Monocle container first if present
        let monocle = ws
            .monocle_container()
            .as_ref()
            .map(|c| Self::from_container(c, !has_focused_float));

        // All tiled containers, focus only if there's no monocle/focused float
        let has_focused_monocle_or_float = has_focused_float || monocle.is_some();
        let tiled = ws.containers().iter().enumerate().map(|(i, c)| {
            let is_focused = !has_focused_monocle_or_float && i == ws.focused_container_idx();
            Self::from_container(c, is_focused)
        });

        // All floating windows
        let floats = ws.floating_windows().iter().map(Self::from_window);
        // All windows
        monocle.into_iter().chain(tiled).chain(floats).collect()
    }

    pub fn from_focused_container(ws: &Workspace) -> Option<Self> {
        if let Some(window) = ws.floating_windows().iter().find(|w| w.is_focused()) {
            return Some(Self::from_window(window));
        }
        if let Some(container) = ws.monocle_container() {
            Some(Self::from_container(container, true))
        } else {
            ws.focused_container()
                .map(|container| Self::from_container(container, true))
        }
    }

    pub fn from_container(container: &Container, is_focused: bool) -> Self {
        Self {
            windows: container.windows().iter().map(WindowInfo::from).collect(),
            focused_window_idx: container.focused_window_idx(),
            is_focused,
            is_locked: container.locked(),
        }
    }

    pub fn from_window(window: &Window) -> Self {
        Self {
            windows: vec![window.into()],
            focused_window_idx: 0,
            is_focused: window.is_focused(),
            is_locked: false, // locked is only container feauture
        }
    }
}

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub title: Option<String>,
    pub icon: Option<ImageIcon>,
}

impl From<&Window> for WindowInfo {
    fn from(value: &Window) -> Self {
        Self {
            title: value.title().ok(),
            icon: ImageIcon::try_load(value.hwnd, || {
                windows_icons::get_icon_by_hwnd(value.hwnd)
                    .or_else(|| windows_icons_fallback::get_icon_by_process_id(value.process_id()))
            }),
        }
    }
}
