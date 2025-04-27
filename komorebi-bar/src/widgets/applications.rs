use super::komorebi::img_to_texture;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use eframe::egui::vec2;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::CornerRadius;
use eframe::egui::FontId;
use eframe::egui::Frame;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::Margin;
use eframe::egui::RichText;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::StrokeKind;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use image::DynamicImage;
use image::RgbaImage;
use komorebi_client::PathExt;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use tracing;
use which::which;

/// Minimum interval between consecutive application launches to prevent accidental spamming.
const MIN_LAUNCH_INTERVAL: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ApplicationsConfig {
    /// Enables or disables the applications widget.
    pub enable: bool,
    /// Whether to show the launch command on hover (optional).
    /// Could be overridden per application. Defaults to `false` if not set.
    pub show_command_on_hover: Option<bool>,
    /// Horizontal spacing between application buttons.
    pub spacing: Option<f32>,
    /// Default display format for all applications (optional).
    /// Could be overridden per application. Defaults to `Icon`.
    pub display: Option<DisplayFormat>,
    /// List of configured applications to display.
    pub items: Vec<AppConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AppConfig {
    /// Whether to enable this application button (optional).
    /// Inherits from the global `Applications` setting if omitted.
    pub enable: Option<bool>,
    /// Whether to show the launch command on hover (optional).
    /// Inherits from the global `Applications` setting if omitted.
    pub show_command_on_hover: Option<bool>,
    /// Display name of the application.
    pub name: String,
    /// Optional icon: a path to an image or a text-based glyph (e.g., from Nerd Fonts).
    /// If not set, and if the `command` is a path to an executable, an icon might be extracted from it.
    /// Note: glyphs require a compatible `font_family`.
    pub icon: Option<String>,
    /// Command to execute (e.g. path to the application or shell command).
    pub command: String,
    /// Display format for this application button (optional). Overrides global format if set.
    pub display: Option<DisplayFormat>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DisplayFormat {
    /// Show only the application icon.
    #[default]
    Icon,
    /// Show only the application name as text.
    Text,
    /// Show both the application icon and name.
    IconAndText,
}

#[derive(Clone, Debug)]
pub struct Applications {
    /// Whether the applications widget is enabled.
    pub enable: bool,
    /// Horizontal spacing between application buttons.
    pub spacing: Option<f32>,
    /// Applications to be rendered in the UI.
    pub items: Vec<App>,
}

impl BarWidget for Applications {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if !self.enable {
            return;
        }

        let icon_config = IconConfig {
            font_id: config.icon_font_id.clone(),
            size: config.icon_font_id.size,
            color: ctx.style().visuals.selection.stroke.color,
        };

        if let Some(spacing) = self.spacing {
            ui.spacing_mut().item_spacing.x = spacing;
        }

        config.apply_on_widget(false, ui, |ui| {
            for app in &mut self.items {
                app.render(ctx, ui, &icon_config);
            }
        });
    }
}

impl From<&ApplicationsConfig> for Applications {
    fn from(applications_config: &ApplicationsConfig) -> Self {
        // Allow immediate launch by initializing last_launch in the past.
        let last_launch = Instant::now() - 2 * MIN_LAUNCH_INTERVAL;
        let mut applications_config = applications_config.clone();
        let items = applications_config
            .items
            .iter_mut()
            .enumerate()
            .map(|(index, app_config)| {
                app_config.command = app_config
                    .command
                    .replace_env()
                    .to_string_lossy()
                    .to_string();

                if let Some(icon) = &mut app_config.icon {
                    *icon = icon.replace_env().to_string_lossy().to_string();
                }

                App {
                    enable: app_config.enable.unwrap_or(applications_config.enable),
                    name: app_config
                        .name
                        .is_empty()
                        .then(|| format!("App {}", index + 1))
                        .unwrap_or_else(|| app_config.name.clone()),
                    icon: Icon::try_from(app_config),
                    command: app_config.command.clone(),
                    display: app_config
                        .display
                        .or(applications_config.display)
                        .unwrap_or_default(),
                    show_command_on_hover: app_config
                        .show_command_on_hover
                        .or(applications_config.show_command_on_hover)
                        .unwrap_or(false),
                    last_launch,
                }
            })
            .collect();

        Self {
            enable: applications_config.enable,
            items,
            spacing: applications_config.spacing,
        }
    }
}

/// A single resolved application entry used at runtime.
#[derive(Clone, Debug)]
pub struct App {
    /// Whether this application is enabled.
    pub enable: bool,
    /// Display name of the application. Defaults to "App N" if not set.
    pub name: String,
    /// Icon to display for this application, if available.
    pub icon: Option<Icon>,
    /// Command to execute when the application is launched.
    pub command: String,
    /// Display format (icon, text, or both).
    pub display: DisplayFormat,
    /// Whether to show the launch command on hover.
    pub show_command_on_hover: bool,
    /// Last time this application was launched (used for cooldown control).
    pub last_launch: Instant,
}

impl App {
    /// Renders the application button in the provided `Ui` context with a given icon size.
    #[inline]
    pub fn render(&mut self, ctx: &Context, ui: &mut Ui, icon_config: &IconConfig) {
        if self.enable
            && SelectableFrame::new(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(4.0);

                    match self.display {
                        DisplayFormat::Icon => self.draw_icon(ctx, ui, icon_config),
                        DisplayFormat::Text => self.draw_name(ui),
                        DisplayFormat::IconAndText => {
                            self.draw_icon(ctx, ui, icon_config);
                            self.draw_name(ui);
                        }
                    }

                    // Add hover text with command information
                    if self.show_command_on_hover {
                        ui.response()
                            .on_hover_text(format!("Launch: {}", self.command));
                    } else {
                        ui.response();
                    }
                })
                .clicked()
        {
            // Launch the application when clicked
            self.launch_if_ready();
        }
    }

    /// Draws the application's icon within the UI if available,
    /// or falls back to a default placeholder icon.
    #[inline]
    fn draw_icon(&self, ctx: &Context, ui: &mut Ui, icon_config: &IconConfig) {
        if let Some(icon) = &self.icon {
            icon.draw(ctx, ui, icon_config);
        } else {
            Icon::draw_fallback(ui, Vec2::splat(icon_config.size));
        }
    }

    /// Displays the application's name as a non-selectable label within the UI.
    #[inline]
    fn draw_name(&self, ui: &mut Ui) {
        ui.add(Label::new(&self.name).selectable(false));
    }

    /// Attempts to launch the specified command in a separate thread if enough time has passed
    /// since the last launch. This prevents repeated launches from rapid consecutive clicks.
    ///
    /// Errors during launch are logged using the `tracing` crate.
    pub fn launch_if_ready(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_launch) < MIN_LAUNCH_INTERVAL {
            return;
        }

        self.last_launch = now;
        let command_string = self.command.clone();
        // Launch the application in a separate thread to avoid blocking the UI
        std::thread::spawn(move || {
            if let Err(e) = Command::new("cmd").args(["/C", &command_string]).spawn() {
                tracing::error!("Failed to launch command '{}': {}", command_string, e);
            }
        });
    }
}

/// Holds decoded image data to be used as an icon in the UI.
#[derive(Clone, Debug)]
pub enum Icon {
    /// RGBA image used for rendering the icon.
    Image(RgbaImage),
    /// Text-based icon, e.g. from a font like Nerd Fonts.
    Text(String),
}

impl Icon {
    /// Attempts to create an `Icon` from the given `AppConfig`.
    /// Loads the image from a specified icon path or extracts it from the application's
    /// executable if the command points to a valid executable file.
    #[inline]
    pub fn try_from(config: &AppConfig) -> Option<Self> {
        if let Some(icon) = config.icon.as_deref().map(str::trim) {
            if !icon.is_empty() {
                let path = Path::new(&icon);
                if path.is_file() {
                    match image::open(path).as_ref().map(DynamicImage::to_rgba8) {
                        Ok(image) => return Some(Icon::Image(image)),
                        Err(err) => {
                            tracing::error!("Failed to load icon from {}, error: {}", icon, err)
                        }
                    }
                } else {
                    return Some(Icon::Text(icon.to_owned()));
                }
            }
        }

        let binary = PathBuf::from(config.command.split(".exe").next()?);
        let path = if binary.is_file() {
            Some(binary)
        } else {
            which(binary).ok()
        };

        match path {
            Some(path) => windows_icons::get_icon_by_path(&path.to_string_lossy())
                .or_else(|| windows_icons_fallback::get_icon_by_path(&path.to_string_lossy()))
                .map(Icon::Image),
            None => None,
        }
    }

    /// Renders the icon in the given `Ui` context with the specified size.
    #[inline]
    pub fn draw(&self, ctx: &Context, ui: &mut Ui, icon_config: &IconConfig) {
        match self {
            Icon::Image(image) => {
                Frame::NONE
                    .inner_margin(Margin::same(ui.style().spacing.button_padding.y as i8))
                    .show(ui, |ui| {
                        ui.add(
                            Image::from(&img_to_texture(ctx, image))
                                .maintain_aspect_ratio(true)
                                .fit_to_exact_size(Vec2::splat(icon_config.size)),
                        );
                    });
            }
            Icon::Text(icon) => {
                let rich_text = RichText::new(icon)
                    .font(icon_config.font_id.clone())
                    .size(icon_config.size)
                    .color(icon_config.color);
                ui.add(Label::new(rich_text).selectable(false));
            }
        }
    }

    /// Draws a fallback icon when the specified icon cannot be loaded.
    /// Displays a simple crossed-out rectangle as a placeholder.
    #[inline]
    pub fn draw_fallback(ui: &mut Ui, icon_size: Vec2) {
        let (response, painter) = ui.allocate_painter(icon_size, Sense::hover());
        let stroke = Stroke::new(1.0, ui.style().visuals.text_color());
        let mut rect = response.rect;
        let rounding = CornerRadius::same((rect.width() * 0.1) as u8);
        rect = rect.shrink(stroke.width);
        let c = rect.center();
        let r = rect.width() / 2.0;
        painter.rect_stroke(rect, rounding, stroke, StrokeKind::Outside);
        painter.line_segment([c - vec2(r, r), c + vec2(r, r)], stroke);
    }
}

/// Configuration structure for icon rendering
#[derive(Clone, Debug)]
pub struct IconConfig {
    /// Font used for text-based icons
    pub font_id: FontId,
    /// Size of the icon
    pub size: f32,
    /// Color of the icon used for text-based icons
    pub color: Color32,
}
