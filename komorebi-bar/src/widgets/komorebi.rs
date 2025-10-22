use super::ImageIcon;
use crate::MAX_LABEL_WIDTH;
use crate::MONITOR_INDEX;
use crate::config::DisplayFormat;
use crate::config::DisplayFormat::*;
use crate::config::WorkspacesDisplayFormat;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::ui::CustomUi;
use crate::widgets::komorebi_layout::KomorebiLayout;
use crate::widgets::widget::BarWidget;
use eframe::egui::Align;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::CornerRadius;
use eframe::egui::FontId;
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
use eframe::egui::text::LayoutJob;
use eframe::egui::vec2;
use komorebi_client::Container;
use komorebi_client::PathExt;
use komorebi_client::Rect;
use komorebi_client::SocketMessage;
use komorebi_client::SocketMessage::*;
use komorebi_client::State;
use komorebi_client::Window;
use komorebi_client::Workspace;
use komorebi_client::WorkspaceLayer;
use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::Result as IoResult;
use std::path::Path;
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
    fn from(cfg: &KomorebiConfig) -> Self {
        let configuration_switcher = cfg.configuration_switcher.clone().map(|mut cs| {
            for location in cs.configurations.values_mut() {
                let path = Path::new(location).replace_env();
                *location = dunce::simplified(&path).to_string_lossy().to_string();
            }
            cs
        });

        Self {
            monitor_info: Rc::new(RefCell::new(MonitorInfo {
                hide_empty_workspaces: cfg
                    .workspaces
                    .map(|w| w.hide_empty_workspaces)
                    .unwrap_or_default(),
                ..Default::default()
            })),
            workspaces: cfg.workspaces.and_then(WorkspacesBar::try_from),
            layout: cfg.layout.clone(),
            focused_container: cfg
                .focused_container
                .and_then(FocusedContainerBar::try_from),
            workspace_layer: cfg.workspace_layer.and_then(WorkspaceLayerBar::try_from),
            locked_container: cfg.locked_container.and_then(LockedContainerBar::try_from),
            configuration_switcher,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Komorebi {
    pub monitor_info: Rc<RefCell<MonitorInfo>>,
    pub workspaces: Option<WorkspacesBar>,
    pub layout: Option<KomorebiLayoutConfig>,
    pub focused_container: Option<FocusedContainerBar>,
    pub workspace_layer: Option<WorkspaceLayerBar>,
    pub locked_container: Option<LockedContainerBar>,
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

impl BarWidget for Komorebi {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        self.render_workspaces(ctx, ui, config);
        self.render_workspace_layer(ctx, ui, config);
        self.render_layout(ctx, ui, config);
        self.render_config_switcher(ui, config);
        self.render_locked_container(ctx, ui, config);
        self.render_focused_container(ctx, ui, config);
    }
}

impl Komorebi {
    /// Renders the workspace bar for the current monitor.
    /// Updates the focused workspace when a workspace is clicked.
    fn render_workspaces(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let monitor_info = &mut *self.monitor_info.borrow_mut();

        let bar = match &mut self.workspaces {
            Some(wg) if !monitor_info.workspaces.is_empty() => wg,
            _ => return,
        };

        bar.text_size = Vec2::splat(config.text_font_id.size);
        bar.icon_size = Vec2::splat(config.icon_font_id.size);

        config.apply_on_widget(false, ui, |ui| {
            for (index, workspace) in monitor_info.workspaces.iter().enumerate() {
                if !workspace.should_show {
                    continue;
                }

                let response = SelectableFrame::new(workspace.is_selected)
                    .show(ui, |ui| (bar.renderer)(bar, ctx, ui, workspace));

                if response.clicked() {
                    let message = FocusMonitorWorkspaceNumber(monitor_info.monitor_index, index);
                    if Self::send_with_mouse_follow_off(monitor_info, message).is_ok() {
                        monitor_info.focused_workspace_idx = Some(index);
                    }
                }
            }
        });
    }

    /// Renders the workspace layer bar for the current monitor.
    /// Handles user clicks to toggle the workspace layer.
    fn render_workspace_layer(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let Some(bar) = &self.workspace_layer else {
            return;
        };

        let monitor_info = &*self.monitor_info.borrow();
        let Some(layer) = monitor_info.focused_workspace_layer() else {
            return;
        };

        if (matches!(layer, WorkspaceLayer::Floating)
            || bar.show_when_tiling && matches!(layer, WorkspaceLayer::Tiling))
        {
            let size = Vec2::splat(config.icon_font_id.size);
            config.apply_on_widget(false, ui, |ui| {
                let layer_frame = SelectableFrame::new(false)
                    .show(ui, |ui| (bar.renderer)(ctx, ui, &layer, size))
                    .on_hover_text(layer.to_string());

                if layer_frame.clicked() {
                    let _ = Self::send_messages(&[
                        FocusMonitorAtCursor,
                        MouseFollowsFocus(false),
                        ToggleWorkspaceLayer,
                        MouseFollowsFocus(monitor_info.mouse_follows_focus),
                    ]);
                }
            });
        }
    }

    /// Renders the configuration switcher bar.
    /// For each available configuration, displays a selectable label.
    /// On click, sends a message to replace the current Komorebi configuration.
    fn render_config_switcher(&mut self, ui: &mut Ui, config: &mut RenderConfig) {
        let configuration_switcher = match &self.configuration_switcher {
            Some(config) if config.enable => config,
            _ => return,
        };
        for (name, location) in configuration_switcher.configurations.iter() {
            let path = Path::new(location);
            if path.is_file() {
                config.apply_on_widget(false, ui, |ui| {
                    let response = SelectableFrame::new(false)
                        .show(ui, |ui| ui.add(Label::new(name).selectable(false)));
                    if response.clicked() {
                        let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned());
                        let _ = Self::send_messages(&[ReplaceConfiguration(path)]);
                    }
                });
            }
        }
    }

    fn render_layout(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if let Some(layout_config) = &self.layout
            && layout_config.enable
        {
            let monitor_info = &mut *self.monitor_info.borrow_mut();
            let workspace_idx = monitor_info.focused_workspace_idx;
            monitor_info
                .layout
                .show(ctx, ui, config, layout_config, workspace_idx);
        }
    }

    /// Renders the locked container bar for the current monitor.
    ///
    /// Shows lock status and allows toggling the lock state when clicked.
    /// Only renders if a focused container exists and either the container is locked
    /// or `show_when_unlocked` is enabled in the bar configuration.
    fn render_locked_container(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let Some(bar) = &self.locked_container else {
            return;
        };

        let monitor_info = &*self.monitor_info.borrow();
        let is_locked = monitor_info
            .focused_container()
            .map(|container| container.is_locked)
            .unwrap_or_default();

        let (icon_font, txt_font) = (config.icon_font_id.clone(), config.text_font_id.clone());
        if (bar.show_when_unlocked || is_locked) && monitor_info.focused_container().is_some() {
            config.apply_on_widget(false, ui, |ui| {
                SelectableFrame::new(false)
                    .show(ui, |ui| {
                        (bar.renderer)(ctx, ui, is_locked, icon_font, txt_font)
                    })
                    .clicked()
                    .then(|| Self::send_messages(&[FocusMonitorAtCursor, ToggleLock]));
            });
        }
    }

    fn render_focused_container(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        let Some(bar) = &self.focused_container else {
            return;
        };
        let monitor_info = &*self.monitor_info.borrow();
        let Some(container) = monitor_info.focused_container() else {
            return;
        };
        config.apply_on_widget(false, ui, |ui| {
            let (len, focused_idx) = (container.windows.len(), container.focused_window_idx);

            for (idx, window) in container.windows.iter().enumerate() {
                let selected = idx == focused_idx && len != 1;
                let text_color = if selected {
                    ctx.style().visuals.selection.stroke.color
                } else {
                    ui.style().visuals.text_color()
                };

                let response = SelectableFrame::new(selected).show(ui, |ui| {
                    (bar.renderer)(bar, ctx, ui, window, text_color, idx == focused_idx)
                });

                if response.clicked() && !selected {
                    let _ = Self::send_with_mouse_follow_off(monitor_info, FocusStackWindow(idx));
                }
            }
        });
    }

    /// Sends a message to Komorebi, temporarily disabling MouseFollowsFocus if it's enabled.
    fn send_with_mouse_follow_off(monitor: &MonitorInfo, message: SocketMessage) -> IoResult<()> {
        let messages: &[SocketMessage] = if monitor.mouse_follows_focus {
            &[MouseFollowsFocus(false), message, MouseFollowsFocus(true)]
        } else {
            &[message]
        };
        Self::send_messages(messages)
    }

    /// Sends a batch of messages to Komorebi, logging errors on failure.
    fn send_messages(messages: &[SocketMessage]) -> IoResult<()> {
        komorebi_client::send_batch(messages).map_err(|err| {
            tracing::error!("Failed to send message(s): {:?}\nError: {}", messages, err);
            err
        })
    }
}

/// Workspace bar with a pre-selected render strategy for efficient
/// workspace display
#[derive(Clone, Debug)]
pub struct WorkspacesBar {
    /// Chosen rendering function for this widget
    renderer: fn(&Self, &Context, &mut Ui, &WorkspaceInfo),
    /// Text size (default: 12.5)
    text_size: Vec2,
    /// Icon size (default: 12.5 * 1.4)
    icon_size: Vec2,
}

impl WorkspacesBar {
    /// Creates a `WorkspacesBar` instance from a workspace configuration.
    ///
    /// Selects a render strategy based on the given display format
    /// for optimal performance. Returns `None` if the widget is disabled.
    fn try_from(value: KomorebiWorkspacesConfig) -> Option<Self> {
        use WorkspacesDisplayFormat::*;
        if !value.enable {
            return None;
        }
        // Selects a render strategy according to the workspace config's display format
        // for better performance
        let renderer: fn(&Self, &Context, &mut Ui, &WorkspaceInfo) =
            match value.display.unwrap_or(DisplayFormat::Text.into()) {
                // 1: Show icons if any, fallback if none | Only hover workspace name
                AllIcons | Existing(DisplayFormat::Icon) => |bar, ctx, ui, ws| {
                    bar.show_icons(ctx, ui, ws)
                        .unwrap_or_else(|| bar.show_fallback_icon(ctx, ui, ws))
                        .on_hover_text(&ws.name);
                },
                // 2: Show icons, with no fallback | Label workspace name (no hover)
                AllIconsAndText | Existing(DisplayFormat::IconAndText) => |bar, ctx, ui, ws| {
                    bar.show_icons(ctx, ui, ws);
                    Self::show_label(ctx, ui, ws);
                },
                // 3: Show icons, fallback if no icons and not selected | Label workspace name if selected else hover
                AllIconsAndTextOnSelected | Existing(DisplayFormat::IconAndTextOnSelected) => {
                    |bar, ctx, ui, ws| {
                        if bar.show_icons(ctx, ui, ws).is_none() && !ws.is_selected {
                            bar.show_fallback_icon(ctx, ui, ws);
                        }
                        if ws.is_selected {
                            Self::show_label(ctx, ui, ws);
                        } else {
                            ui.response().on_hover_text(&ws.name);
                        }
                    }
                }
                // 4: Show icons if selected and has icons (no fallback) | Label workspace name
                Existing(DisplayFormat::TextAndIconOnSelected) => |bar, ctx, ui, ws| {
                    if ws.is_selected {
                        bar.show_icons(ctx, ui, ws);
                    }
                    Self::show_label(ctx, ui, ws);
                },
                // 5: Never show icon (no icons at all) | Label workspace name always
                Existing(DisplayFormat::Text) => |_, ctx, ui, ws| {
                    Self::show_label(ctx, ui, ws);
                },
            };

        Some(Self {
            renderer,
            icon_size: Vec2::splat(12.5),
            text_size: Vec2::splat(12.5 * 1.4),
        })
    }

    /// Draws all window icons for the workspace, using larger size for the focused container.
    /// Returns response if icons exist, or None.
    fn show_icons(&self, ctx: &Context, ui: &mut Ui, ws: &WorkspaceInfo) -> Option<Response> {
        ws.has_icons.then(|| {
            Frame::NONE
                .inner_margin(Margin::same(ui.style().spacing.button_padding.y as i8))
                .show(ui, |ui| {
                    for container in &ws.containers {
                        for icon in container.windows.iter().filter_map(|win| win.icon.as_ref()) {
                            ui.add(
                                Image::from(&icon.texture(ctx))
                                    .maintain_aspect_ratio(true)
                                    .fit_to_exact_size(if container.is_focused {
                                        self.icon_size
                                    } else {
                                        self.text_size
                                    }),
                            );
                        }
                    }
                })
                .response
        })
    }

    /// Draws a fallback icon (a rectangle with a diagonal) for the workspace.
    fn show_fallback_icon(&self, ctx: &Context, ui: &mut Ui, ws: &WorkspaceInfo) -> Response {
        let (response, painter) = ui.allocate_painter(self.icon_size, Sense::hover());
        let stroke: Stroke = Stroke::new(
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
        let c = rect.center();
        let r = rect.width() / 2.0;
        painter.rect_stroke(rect, rounding, stroke, StrokeKind::Outside);
        painter.line_segment([c - vec2(r, r), c + vec2(r, r)], stroke);
        response
    }

    /// Shows the workspace label (colored if selected).
    fn show_label(ctx: &Context, ui: &mut Ui, ws: &WorkspaceInfo) -> Response {
        if ws.is_selected {
            let text = RichText::new(&ws.name).color(ctx.style().visuals.selection.stroke.color);
            ui.add(Label::new(text).selectable(false))
        } else {
            ui.add(Label::new(&ws.name).selectable(false))
        }
    }
}

/// Structure for displaying the focused container's windows in the bar.
/// Uses a pre-selected rendering strategy (based on config) for efficient display
#[derive(Clone, Debug)]
pub struct FocusedContainerBar {
    /// Chosen rendering function for this widget
    renderer: fn(&Self, &Context, &mut Ui, &WindowInfo, Color32, focused: bool),
    /// Icon size (default: 12.5 * 1.4)
    icon_size: Vec2,
}

impl FocusedContainerBar {
    /// Creates a `FocusedContainerBar` from the given configuration.
    ///
    /// Selects a render strategy based on the display format or legacy icon setting.
    /// Returns `None` if the widget is disabled.
    fn try_from(value: KomorebiFocusedContainerConfig) -> Option<Self> {
        if !value.enable {
            return None;
        }
        // Handle legacy setting - convert show_icon to display format
        let format = value
            .display
            .unwrap_or(if value.show_icon.unwrap_or(false) {
                IconAndText
            } else {
                Text
            });

        // Select renderer strategy based on display format for better performance
        let renderer: fn(&FocusedContainerBar, &Context, &mut Ui, &WindowInfo, Color32, bool) =
            match format {
                Icon => |_self, ctx, ui, info, _color, _focused| {
                    FocusedContainerBar::show_icon::<true>(_self, ctx, ui, info);
                },
                Text => |_self, _ctx, ui, info, color, _focused| {
                    FocusedContainerBar::show_title(_self, ui, info, color);
                },
                IconAndText => |_self, ctx, ui, info, color, _focused| {
                    FocusedContainerBar::show_icon::<false>(_self, ctx, ui, info);
                    FocusedContainerBar::show_title(_self, ui, info, color);
                },
                IconAndTextOnSelected => |_self, ctx, ui, info, color, focused| {
                    FocusedContainerBar::show_icon::<false>(_self, ctx, ui, info);
                    if focused {
                        FocusedContainerBar::show_title(_self, ui, info, color);
                    }
                },
                TextAndIconOnSelected => |_self, ctx, ui, info, color, focused| {
                    if focused {
                        FocusedContainerBar::show_icon::<false>(_self, ctx, ui, info);
                    }
                    FocusedContainerBar::show_title(_self, ui, info, color);
                },
            };

        Some(FocusedContainerBar {
            renderer,
            icon_size: Vec2::splat(12.5 * 1.4),
        })
    }

    /// Draws window icon(s) at configured size; adds hover text if HOVEL is true.
    fn show_icon<const HOVEL: bool>(&self, ctx: &Context, ui: &mut Ui, info: &WindowInfo) {
        let inner_response = Frame::NONE
            .inner_margin(Margin::same(ui.style().spacing.button_padding.y as i8))
            .show(ui, |ui| {
                for icon in info.icon.iter() {
                    let img = Image::from(&icon.texture(ctx)).maintain_aspect_ratio(true);
                    ui.add(img.fit_to_exact_size(self.icon_size));
                }
            });
        if HOVEL && let Some(title) = &info.title {
            inner_response.response.on_hover_text(title);
        }
    }

    /// Renders the window title label in color, truncating if needed.
    fn show_title(&self, ui: &mut Ui, info: &WindowInfo, color: Color32) {
        if let Some(title) = &info.title {
            let text = Label::new(RichText::new(title).color(color)).selectable(false);

            let x = MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32;
            let y = ui.available_height();
            CustomUi(ui).add_sized_left_to_right(Vec2::new(x, y), text.truncate());
        }
    }
}

/// Workspace layer bar with a pre-selected render strategy for efficient
/// layer display
#[derive(Clone, Debug)]
pub struct WorkspaceLayerBar {
    /// Show the widget even if the layer is Tiling
    show_when_tiling: bool,
    /// Chosen rendering function for this widget
    renderer: fn(&Context, &mut Ui, &WorkspaceLayer, Vec2),
}

impl WorkspaceLayerBar {
    /// Creates a `WorkspaceLayerBar` from the given configuration.
    ///
    /// Selects a render strategy based on the display format.
    /// Returns `None` if the widget is disabled.
    fn try_from(value: KomorebiWorkspaceLayerConfig) -> Option<Self> {
        if !value.enable {
            return None;
        }
        let display_format = value.display.unwrap_or(Text);

        // Select renderer strategy based on display format for better performance
        let renderer: fn(&Context, &mut Ui, &WorkspaceLayer, Vec2) = match display_format {
            Icon => Self::draw_layer_icon,
            Text => |_ctx, ui, layer, _size| {
                ui.add(Label::new(layer.to_string()).selectable(false));
            },
            _ => |ctx, ui, layer, size| {
                Self::draw_layer_icon(ctx, ui, layer, size);
                ui.add(Label::new(layer.to_string()).selectable(false));
            },
        };

        Some(Self {
            show_when_tiling: value.show_when_tiling.unwrap_or_default(),
            renderer,
        })
    }

    /// Draws an icon representing the current workspace layer.
    fn draw_layer_icon(ctx: &Context, ui: &mut Ui, layer: &WorkspaceLayer, size: Vec2) {
        if matches!(layer, WorkspaceLayer::Tiling) {
            let (response, painter) = ui.allocate_painter(size, Sense::hover());
            let color = ctx.style().visuals.selection.stroke.color;
            let stroke = Stroke::new(1.0, color);
            let mut rect = response.rect;
            let corner = CornerRadius::same((rect.width() * 0.1) as u8);
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
            painter.rect_stroke(rect_right, corner, stroke, StrokeKind::Outside);
        } else {
            let (response, painter) = ui.allocate_painter(size, Sense::hover());
            let color = ctx.style().visuals.selection.stroke.color;
            let stroke = Stroke::new(1.0, color);
            let mut rect = response.rect;
            let corner = CornerRadius::same((rect.width() * 0.1) as u8);
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
            painter.rect_stroke(rect_right, corner, stroke, StrokeKind::Outside);
        }
    }
}

/// Locked container bar for displaying and interacting with container lock state
#[derive(Clone, Debug)]
pub struct LockedContainerBar {
    /// Show the widget even when unlocked
    show_when_unlocked: bool,
    /// Chosen rendering function for this widget
    renderer: fn(&Context, &mut Ui, bool, FontId, FontId),
}

impl LockedContainerBar {
    /// Creates a `LockedContainerBar` from the given configuration.
    ///
    /// Selects a render strategy based on the display format.
    /// Returns `None` if the widget is disabled.
    fn try_from(value: KomorebiLockedContainerConfig) -> Option<Self> {
        if !value.enable {
            return None;
        }
        let display_format = value.display.unwrap_or(DisplayFormat::Text);

        // Select renderer strategy based on display format for better performance
        let renderer: fn(&Context, &mut Ui, bool, FontId, FontId) = match display_format {
            DisplayFormat::Icon => |ctx, ui, is_locked, icon_font, _txt_font| {
                let layout_job = Self::icon_layout(ctx, is_locked, icon_font);
                ui.add(Label::new(layout_job).selectable(false));
            },
            DisplayFormat::Text => |ctx, ui, is_locked, _icon_font, txt_font| {
                let mut layout_job = LayoutJob::default();
                Self::append_text(&mut layout_job, ctx, is_locked, txt_font);
                ui.add(Label::new(layout_job).selectable(false));
            },
            _ => |ctx, ui: &mut Ui, is_locked, icon_font, txt_font| {
                let mut layout_job = Self::icon_layout(ctx, is_locked, icon_font);
                Self::append_text(&mut layout_job, ctx, is_locked, txt_font);
                ui.add(Label::new(layout_job).selectable(false));
            },
        };

        Some(Self {
            show_when_unlocked: value.show_when_unlocked.unwrap_or_default(),
            renderer,
        })
    }

    /// Builds a layout job for the lock/unlock icon,
    /// using the provided font and current lock state.
    fn icon_layout(ctx: &Context, is_locked: bool, icon_font: FontId) -> LayoutJob {
        LayoutJob::simple(
            if is_locked {
                egui_phosphor::regular::LOCK_KEY.to_string()
            } else {
                egui_phosphor::regular::LOCK_SIMPLE_OPEN.to_string()
            },
            icon_font,
            ctx.style().visuals.selection.stroke.color,
            100.0,
        )
    }

    /// Appends lock/unlock status text to the given layout job,
    /// using the specified font and current lock state.
    fn append_text(job: &mut LayoutJob, ctx: &Context, is_locked: bool, txt_font: FontId) {
        job.append(
            if is_locked { "Locked" } else { "Unlocked" },
            10.0,
            TextFormat {
                font_id: txt_font,
                color: ctx.style().visuals.text_color(),
                valign: Align::Center,
                ..Default::default()
            },
        )
    }
}

/// Represents the full state of a single monitor for the Komorebi bar/UI.
///
/// Includes all workspaces, containers, windows, layout, focus,
/// display options, and monitor indices. Used to render the current monitor
/// state in UI widgets.
///
/// Updated whenever Komorebi state changes.
#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub workspaces: Vec<WorkspaceInfo>,
    pub layout: KomorebiLayout,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    pub monitor_index: usize,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    pub focused_workspace_idx: Option<usize>,
    pub show_all_icons: bool,
    pub hide_empty_workspaces: bool,
}

impl Default for MonitorInfo {
    fn default() -> Self {
        Self {
            workspaces: Vec::new(),
            layout: KomorebiLayout::Default(komorebi_client::DefaultLayout::BSP),
            mouse_follows_focus: true,
            work_area_offset: None,
            monitor_index: MONITOR_INDEX.load(Ordering::SeqCst),
            monitor_usr_idx_map: HashMap::new(),
            focused_workspace_idx: None,
            show_all_icons: false,
            hide_empty_workspaces: false,
        }
    }
}

impl MonitorInfo {
    /// Returns a reference to the currently focused workspace, if any.
    pub fn focused_workspace(&self) -> Option<&WorkspaceInfo> {
        self.workspaces.get(self.focused_workspace_idx?)
    }

    /// Returns the layer of the focused workspace, if available.
    pub fn focused_workspace_layer(&self) -> Option<WorkspaceLayer> {
        self.focused_workspace().map(|ws| ws.layer)
    }

    /// Returns the focused container within the focused workspace, if any.
    pub fn focused_container(&self) -> Option<&ContainerInfo> {
        self.focused_workspace()
            .and_then(WorkspaceInfo::focused_container)
    }
}

impl MonitorInfo {
    pub fn update_from_self(&mut self, config: &Self) {
        self.hide_empty_workspaces = config.hide_empty_workspaces;
    }

    /// Updates monitor state from the given State, setting all fields based on the selected
    /// monitor and its workspaces
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
        self.work_area_offset = monitor.work_area_offset;
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

    /// Builds an iterator of WorkspaceInfo for the monitor.
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
                    .name
                    .to_owned()
                    .unwrap_or_else(|| format!("{}", index + 1)),
                focused_container_idx: containers.iter().position(|c| c.is_focused),
                has_icons: containers
                    .iter()
                    .any(|container| container.windows.iter().any(|window| window.icon.is_some())),
                containers,
                layer: ws.layer,
                should_show: !hide_empty_ws || focused_ws_idx == Some(index) || !ws.is_empty(),
                is_selected: focused_ws_idx == Some(index),
            }
        })
    }

    /// Determines the current layout of the focused workspace
    fn resolve_layout(focused_ws: &Workspace, is_paused: bool) -> KomorebiLayout {
        if focused_ws.monocle_container.is_some() {
            KomorebiLayout::Monocle
        } else if !focused_ws.tile {
            KomorebiLayout::Floating
        } else if is_paused {
            KomorebiLayout::Paused
        } else {
            match focused_ws.layout {
                komorebi_client::Layout::Default(layout) => KomorebiLayout::Default(layout),
                komorebi_client::Layout::Custom(_) => KomorebiLayout::Custom,
            }
        }
    }
}

/// Describes the state of a single workspace on a monitor.
///
/// Contains the workspace name, its containers (tiled, floating,
/// monocle), focus state, layer (tiling/floating), and display
/// flags for UI rendering.
#[derive(Clone, Debug)]
pub struct WorkspaceInfo {
    pub name: String,
    pub containers: Vec<ContainerInfo>,
    pub focused_container_idx: Option<usize>,
    pub layer: WorkspaceLayer,
    pub should_show: bool,
    pub is_selected: bool,
    pub has_icons: bool,
}

impl WorkspaceInfo {
    /// Returns a reference to the focused container in this workspace, if any.
    pub fn focused_container(&self) -> Option<&ContainerInfo> {
        self.containers.get(self.focused_container_idx?)
    }
}

/// Holds information about a window container (tiled, floating, or monocle)
/// within a workspace.
///
/// Includes a list of windows, the focused window index, and flags for focus
/// and lock status.
#[derive(Clone, Debug)]
pub struct ContainerInfo {
    pub windows: Vec<WindowInfo>,
    pub focused_window_idx: usize,
    pub is_focused: bool,
    pub is_locked: bool,
}

impl ContainerInfo {
    /// Returns all containers for the given workspace in the following order:
    ///
    /// 1. The monocle container (if present) is included first.
    /// 2. All tiled containers are included next.
    /// 3. All floating windows are added last, each as a separate container.
    ///
    /// Function ensures only one container is marked as focused, prioritizing
    /// floating → monocle → tiled.
    pub fn from_all_containers(ws: &Workspace) -> Vec<Self> {
        let has_focused_float = ws.floating_windows().iter().any(|w| w.is_focused());

        // Monocle container first if present
        let monocle = ws
            .monocle_container
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

    /// Creates a `ContainerInfo` for the currently focused item in the workspace.
    ///
    /// The function checks focus in the following order:
    /// 1. Focused floating window
    /// 2. Monocle container
    /// 3. Focused tiled container
    pub fn from_focused_container(ws: &Workspace) -> Option<Self> {
        if let Some(window) = ws.floating_windows().iter().find(|w| w.is_focused()) {
            return Some(Self::from_window(window));
        }
        if let Some(container) = &ws.monocle_container {
            Some(Self::from_container(container, true))
        } else {
            ws.focused_container()
                .map(|container| Self::from_container(container, true))
        }
    }

    /// Creates a `ContainerInfo` from a given container.
    pub fn from_container(container: &Container, is_focused: bool) -> Self {
        Self {
            windows: container.windows().iter().map(WindowInfo::from).collect(),
            focused_window_idx: container.focused_window_idx(),
            is_focused,
            is_locked: container.locked,
        }
    }

    /// Creates a `ContainerInfo` from a single floating window.
    /// The window becomes the only entry in `windows`, is marked as focused
    /// if applicable, and `is_locked` is set to false.
    pub fn from_window(window: &Window) -> Self {
        Self {
            windows: vec![window.into()],
            focused_window_idx: 0,
            is_focused: window.is_focused(),
            is_locked: false, // locked is only container feature
        }
    }
}

/// Stores basic information about a single window in a container.
/// Contains the window's title and its icon, if available.
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
