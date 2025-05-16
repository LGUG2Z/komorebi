use super::ImageIcon;
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
use komorebi_client::PathExt;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
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
        let items = applications_config
            .items
            .iter()
            .enumerate()
            .map(|(index, config)| {
                let command = UserCommand::new(&config.command);

                App {
                    enable: config.enable.unwrap_or(applications_config.enable),
                    #[allow(clippy::obfuscated_if_else)]
                    name: config
                        .name
                        .is_empty()
                        .then(|| format!("App {}", index + 1))
                        .unwrap_or_else(|| config.name.clone()),
                    icon: Icon::try_from_path(config.icon.as_deref())
                        .or_else(|| Icon::try_from_command(&command)),
                    command,
                    display: config
                        .display
                        .or(applications_config.display)
                        .unwrap_or_default(),
                    show_command_on_hover: config
                        .show_command_on_hover
                        .or(applications_config.show_command_on_hover)
                        .unwrap_or(false),
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
    pub command: UserCommand,
    /// Display format (icon, text, or both).
    pub display: DisplayFormat,
    /// Whether to show the launch command on hover.
    pub show_command_on_hover: bool,
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
                    let response = ui.response();
                    if self.show_command_on_hover {
                        response.on_hover_text(format!("Launch: {}", self.command.as_ref()));
                    }
                })
                .clicked()
        {
            // Launch the application when clicked
            self.command.launch_if_ready();
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
}

/// Holds image/text data to be used as an icon in the UI.
/// This represents source icon data before rendering.
#[derive(Clone, Debug)]
pub enum Icon {
    /// RGBA image used for rendering the icon.
    Image(ImageIcon),
    /// Text-based icon, e.g. from a font like Nerd Fonts.
    Text(String),
}

impl Icon {
    /// Attempts to create an [`Icon`] from a string path or text glyph/glyphs.
    ///
    /// - Environment variables in the path are resolved using [`PathExt::replace_env`].
    /// - Uses [`ImageIcon::try_load`] to load and cache the icon image based on the resolved path.
    /// - If the path is invalid but the string is non-empty, it is interpreted as a text-based icon and
    ///   returned as [`Icon::Text`].
    /// - Returns `None` if the input is empty, `None`, or image loading fails.
    #[inline]
    pub fn try_from_path(icon: Option<&str>) -> Option<Self> {
        let icon = icon.map(str::trim)?;
        if icon.is_empty() {
            return None;
        }

        let path = icon.replace_env();
        if !path.is_file() {
            return Some(Icon::Text(icon.to_owned()));
        }

        let image_icon = ImageIcon::try_load(path.as_ref(), || match image::open(&path) {
            Ok(img) => Some(img),
            Err(err) => {
                tracing::error!("Failed to load icon from {:?}, error: {}", path, err);
                None
            }
        })?;

        Some(Icon::Image(image_icon))
    }

    /// Attempts to create an [`Icon`] by extracting an image from the executable path of a [`UserCommand`].
    ///
    /// - Uses [`ImageIcon::try_load`] to load and cache the icon image based on the resolved executable path.
    /// - Returns [`Icon::Image`] if an icon is successfully extracted.
    /// - Returns `None` if the executable path is unavailable or icon extraction fails.
    #[inline]
    pub fn try_from_command(command: &UserCommand) -> Option<Self> {
        let path = command.get_executable()?;
        let image_icon = ImageIcon::try_load(path.as_ref(), || {
            let path_str = path.to_str()?;
            windows_icons::get_icon_by_path(path_str)
                .or_else(|| windows_icons_fallback::get_icon_by_path(path_str))
        })?;
        Some(Icon::Image(image_icon))
    }

    /// Renders the icon in the given [`Ui`] using the provided [`IconConfig`].
    #[inline]
    pub fn draw(&self, ctx: &Context, ui: &mut Ui, icon_config: &IconConfig) {
        match self {
            Icon::Image(image_icon) => {
                Frame::NONE
                    .inner_margin(Margin::same(ui.style().spacing.button_padding.y as i8))
                    .show(ui, |ui| {
                        ui.add(
                            Image::from_texture(&image_icon.texture(ctx))
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

/// A structure to manage command execution with cooldown prevention.
#[derive(Clone, Debug)]
pub struct UserCommand {
    /// The command string to execute
    pub command: Arc<str>,
    /// Last time this command was executed (used for cooldown control)
    pub last_launch: Instant,
}

impl AsRef<str> for UserCommand {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.command
    }
}

impl UserCommand {
    /// Creates a new [`UserCommand`] with environment variables in the command path
    /// resolved using [`PathExt::replace_env`].
    #[inline]
    pub fn new(command: &str) -> Self {
        // Allow immediate launch by initializing last_launch in the past
        let last_launch = Instant::now() - 2 * MIN_LAUNCH_INTERVAL;

        Self {
            command: Arc::from(command.replace_env().to_str().unwrap_or_default()),
            last_launch,
        }
    }

    /// Attempts to resolve the executable path from the command string.
    ///
    /// Resolution logic:
    /// - Splits the command by ".exe" and checks if the first part is an existing file.
    /// - If not, attempts to locate the binary using [`which`] on this name.
    /// - If still unresolved, takes the first word (separated by whitespace) and attempts
    ///   to find it in the system `PATH` using [`which`].
    ///
    /// Returns `None` if no executable path can be determined.
    #[inline]
    pub fn get_executable(&self) -> Option<Cow<'_, Path>> {
        if let Some(binary) = self.command.split(".exe").next().map(Path::new) {
            if binary.is_file() {
                return Some(Cow::Borrowed(binary));
            } else if let Ok(binary) = which(binary) {
                return Some(Cow::Owned(binary));
            }
        }

        which(self.command.split(' ').next()?).ok().map(Cow::Owned)
    }

    /// Attempts to launch the specified command in a separate thread if enough time has passed
    /// since the last launch. This prevents repeated launches from rapid consecutive clicks.
    ///
    /// Errors during launch are logged using the `tracing` crate.
    pub fn launch_if_ready(&mut self) {
        let now = Instant::now();
        // Check if enough time has passed since the last launch
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
