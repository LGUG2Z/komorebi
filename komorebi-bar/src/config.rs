use crate::AUTO_SELECT_FILL_COLOUR;
use crate::AUTO_SELECT_TEXT_COLOUR;
use crate::DEFAULT_PADDING;
use crate::bar::exec_powershell;
use crate::render::Grouping;
use crate::render::RenderConfig;
use crate::render::RenderExt;
use crate::widgets::widget::WidgetConfig;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::TextBuffer;
use eframe::egui::TextStyle;
use komorebi_client::PathExt;
use komorebi_client::Rect;
use komorebi_client::SocketMessage;
use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// The `komorebi.bar.json` configuration file reference for `v0.1.42`
pub struct KomobarConfig {
    /// Bar height
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 50)))]
    pub height: Option<f32>,
    /// Bar padding. Use one value for all sides or use a grouped padding for horizontal and/or
    /// vertical definition which can each take a single value for a symmetric padding or two
    /// values for each side, i.e.:
    /// ```json
    /// "padding": {
    ///     "horizontal": 10
    /// }
    /// ```
    /// or:
    /// ```json
    /// "padding": {
    ///     "horizontal": [left, right]
    /// }
    /// ```
    /// You can also set individual padding on each side like this:
    /// ```json
    /// "padding": {
    ///     "top": 10,
    ///     "bottom": 10,
    ///     "left": 10,
    ///     "right": 10,
    /// }
    /// ```
    /// By default, padding is set to 10 on all sides.
    pub padding: Option<Padding>,
    /// Bar margin. Use one value for all sides or use a grouped margin for horizontal and/or
    /// vertical definition which can each take a single value for a symmetric margin or two
    /// values for each side, i.e.:
    /// ```json
    /// "margin": {
    ///     "horizontal": 10
    /// }
    /// ```
    /// or:
    /// ```json
    /// "margin": {
    ///     "vertical": [top, bottom]
    /// }
    /// ```
    /// You can also set individual margin on each side like this:
    /// ```json
    /// "margin": {
    ///     "top": 10,
    ///     "bottom": 10,
    ///     "left": 10,
    ///     "right": 10,
    /// }
    /// ```
    /// By default, margin is set to 0 on all sides.
    pub margin: Option<Margin>,
    /// Bar positioning options
    #[serde(alias = "viewport")]
    pub position: Option<PositionConfig>,
    /// Frame options (see: https://docs.rs/egui/latest/egui/containers/frame/struct.Frame.html)
    pub frame: Option<FrameConfig>,
    /// The monitor index or the full monitor options
    #[cfg_attr(feature = "schemars", schemars(extend("default" = MonitorConfigOrIndex::Index(0))))]
    pub monitor: Option<MonitorConfigOrIndex>,
    /// Font family
    pub font_family: Option<String>,
    /// Font size
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 12.5)))]
    pub font_size: Option<f32>,
    /// Scale of the icons relative to the font_size [[1.0-2.0]]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 1.4)))]
    pub icon_scale: Option<f32>,
    /// Max label width before text truncation
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 400.0)))]
    pub max_label_width: Option<f32>,
    /// Theme
    pub theme: Option<KomobarTheme>,
    /// Alpha value for the color transparency [[0-255]]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 200)))]
    pub transparency_alpha: Option<u8>,
    /// Spacing between widgets
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 10.0)))]
    pub widget_spacing: Option<f32>,
    /// Visual grouping for widgets
    pub grouping: Option<Grouping>,
    /// Options for mouse interaction on the bar
    pub mouse: Option<MouseConfig>,
    /// Left side widgets (ordered left-to-right)
    pub left_widgets: Vec<WidgetConfig>,
    /// Center widgets (ordered left-to-right)
    pub center_widgets: Option<Vec<WidgetConfig>>,
    /// Right side widgets (ordered left-to-right)
    pub right_widgets: Vec<WidgetConfig>,
}

impl KomobarConfig {
    pub fn aliases(raw: &str) {
        let mut map = HashMap::new();
        map.insert("position", ["viewport"]);
        map.insert("end", ["inner_frame"]);

        let mut display = false;

        for aliases in map.values() {
            for a in aliases {
                if raw.contains(a) {
                    display = true;
                }
            }
        }

        if display {
            println!(
                "\nYour bar configuration file contains some options that have been renamed or deprecated:\n"
            );
            for (canonical, aliases) in map {
                for alias in aliases {
                    if raw.contains(alias) {
                        println!(r#""{alias}" is now "{canonical}""#);
                    }
                }
            }
        }
    }

    pub fn show_all_icons_on_komorebi_workspace(widgets: &[WidgetConfig]) -> bool {
        widgets
            .iter()
            .any(|w| matches!(w, WidgetConfig::Komorebi(config) if config.workspaces.is_some_and(|w| w.enable && w.display.is_some_and(|s| matches!(s,
            WorkspacesDisplayFormat::AllIcons
            | WorkspacesDisplayFormat::AllIconsAndText
            | WorkspacesDisplayFormat::AllIconsAndTextOnSelected)))))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Position configuration
pub struct PositionConfig {
    /// The desired starting position of the bar (0,0 = top left of the screen)
    #[serde(alias = "position")]
    pub start: Option<Position>,
    /// The desired size of the bar from the starting position (usually monitor width x desired height)
    #[serde(alias = "inner_size")]
    pub end: Option<Position>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Frame configuration
pub struct FrameConfig {
    /// Margin inside the painted frame
    pub inner_margin: Position,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Monitor configuration or monitor index
pub enum MonitorConfigOrIndex {
    /// The monitor index where you want the bar to show
    Index(usize),
    /// The full monitor options with the index and an optional work_area_offset
    MonitorConfig(MonitorConfig),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Monitor configuration
pub struct MonitorConfig {
    /// Komorebi monitor index of the monitor on which to render the bar
    pub index: usize,
    /// Automatically apply a work area offset for this monitor to accommodate the bar
    pub work_area_offset: Option<Rect>,
}

pub type Padding = SpacingKind;
pub type Margin = SpacingKind;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
// WARNING: To any developer messing with this code in the future: The order here matters!
// `Grouped` needs to come last, otherwise serde might mistaken an `IndividualSpacingConfig` for a
// `GroupedSpacingConfig` with both `vertical` and `horizontal` set to `None` ignoring the
// individual values.
/// Spacing kind
pub enum SpacingKind {
    /// Spacing applied to all sides
    All(f32),
    /// Individual spacing applied to each side
    Individual(IndividualSpacingConfig),
    /// Grouped vertical and horizontal spacing
    Grouped(GroupedSpacingConfig),
}

impl SpacingKind {
    pub fn to_individual(&self, default: f32) -> IndividualSpacingConfig {
        match self {
            SpacingKind::All(m) => IndividualSpacingConfig::all(*m),
            SpacingKind::Grouped(grouped_spacing_config) => {
                let vm = grouped_spacing_config.vertical.as_ref().map_or(
                    IndividualSpacingConfig::vertical(default),
                    |vm| match vm {
                        GroupedSpacingOptions::Symmetrical(m) => {
                            IndividualSpacingConfig::vertical(*m)
                        }
                        GroupedSpacingOptions::Split(tm, bm) => {
                            IndividualSpacingConfig::vertical(*tm).bottom(*bm)
                        }
                    },
                );
                let hm = grouped_spacing_config.horizontal.as_ref().map_or(
                    IndividualSpacingConfig::horizontal(default),
                    |hm| match hm {
                        GroupedSpacingOptions::Symmetrical(m) => {
                            IndividualSpacingConfig::horizontal(*m)
                        }
                        GroupedSpacingOptions::Split(lm, rm) => {
                            IndividualSpacingConfig::horizontal(*lm).right(*rm)
                        }
                    },
                );
                IndividualSpacingConfig {
                    top: vm.top,
                    bottom: vm.bottom,
                    left: hm.left,
                    right: hm.right,
                }
            }
            SpacingKind::Individual(m) => *m,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Grouped vertical and horizontal spacing
pub struct GroupedSpacingConfig {
    /// Vertical grouped spacing
    pub vertical: Option<GroupedSpacingOptions>,
    /// Horizontal grouped spacing
    pub horizontal: Option<GroupedSpacingOptions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Grouped spacing options
pub enum GroupedSpacingOptions {
    /// Symmetrical grouped spacing
    Symmetrical(f32),
    /// Split grouped spacing
    Split(f32, f32),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Individual spacing configuration
pub struct IndividualSpacingConfig {
    /// Spacing for the top
    pub top: f32,
    /// Spacing for the bottom
    pub bottom: f32,
    /// Spacing for the left
    pub left: f32,
    /// Spacing for the right
    pub right: f32,
}

#[allow(dead_code)]
impl IndividualSpacingConfig {
    pub const ZERO: Self = IndividualSpacingConfig {
        top: 0.0,
        bottom: 0.0,
        left: 0.0,
        right: 0.0,
    };

    pub fn all(value: f32) -> Self {
        IndividualSpacingConfig {
            top: value,
            bottom: value,
            left: value,
            right: value,
        }
    }

    pub fn horizontal(value: f32) -> Self {
        IndividualSpacingConfig {
            top: 0.0,
            bottom: 0.0,
            left: value,
            right: value,
        }
    }

    pub fn vertical(value: f32) -> Self {
        IndividualSpacingConfig {
            top: value,
            bottom: value,
            left: 0.0,
            right: 0.0,
        }
    }

    pub fn top(self, value: f32) -> Self {
        IndividualSpacingConfig { top: value, ..self }
    }

    pub fn bottom(self, value: f32) -> Self {
        IndividualSpacingConfig {
            bottom: value,
            ..self
        }
    }

    pub fn left(self, value: f32) -> Self {
        IndividualSpacingConfig {
            left: value,
            ..self
        }
    }

    pub fn right(self, value: f32) -> Self {
        IndividualSpacingConfig {
            right: value,
            ..self
        }
    }
}

pub fn get_individual_spacing(
    default: f32,
    spacing: &Option<SpacingKind>,
) -> IndividualSpacingConfig {
    spacing
        .as_ref()
        .map_or(IndividualSpacingConfig::all(default), |s| {
            s.to_individual(default)
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Mouse message
pub enum MouseMessage {
    /// Send a message to the komorebi client.
    /// By default, a batch of messages are sent in the following order:
    /// FocusMonitorAtCursor =>
    /// MouseFollowsFocus(false) =>
    /// {message} =>
    /// MouseFollowsFocus({original.value})
    ///
    /// Example:
    /// ```json
    /// "on_extra2_click": {
    ///   "message": {
    ///     "type": "NewWorkspace"
    ///   }
    /// },
    /// ```
    /// or:
    /// ```json
    /// "on_middle_click": {
    ///   "focus_monitor_at_cursor": false,
    ///   "ignore_mouse_follows_focus": false,
    ///   "message": {
    ///     "type": "TogglePause"
    ///   }
    /// }
    /// ```
    /// or:
    /// ```json
    /// "on_scroll_up": {
    ///   "message": {
    ///     "type": "CycleFocusWorkspace",
    ///     "content": "Previous"
    ///   }
    /// }
    /// ```
    Komorebi(KomorebiMouseMessage),
    /// Execute a custom command.
    /// CMD (%variable%), Bash ($variable) and PowerShell ($Env:variable) variables will be resolved.
    /// Example: `komorebic toggle-pause`
    Command(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi socket mouse message
pub struct KomorebiMouseMessage {
    /// Send the FocusMonitorAtCursor message
    #[cfg_attr(feature = "schemars", schemars(extend("default" = true)))]
    pub focus_monitor_at_cursor: Option<bool>,
    /// Wrap the {message} with a MouseFollowsFocus(false) and MouseFollowsFocus({original.value}) message
    #[cfg_attr(feature = "schemars", schemars(extend("default" = true)))]
    pub ignore_mouse_follows_focus: Option<bool>,
    /// The message to send to the komorebi client
    pub message: komorebi_client::SocketMessage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Mouse configuration
pub struct MouseConfig {
    /// Command to send on primary/left double button click
    pub on_primary_double_click: Option<MouseMessage>,
    /// Command to send on secondary/right button click
    pub on_secondary_click: Option<MouseMessage>,
    /// Command to send on middle button click
    pub on_middle_click: Option<MouseMessage>,
    /// Command to send on extra1/back button click
    pub on_extra1_click: Option<MouseMessage>,
    /// Command to send on extra2/forward button click
    pub on_extra2_click: Option<MouseMessage>,

    /// Defines how many points a user needs to scroll vertically to make a "tick" on a mouse/touchpad/touchscreen
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 30.0)))]
    pub vertical_scroll_threshold: Option<f32>,
    /// Command to send on scrolling up (every tick)
    pub on_scroll_up: Option<MouseMessage>,
    /// Command to send on scrolling down (every tick)
    pub on_scroll_down: Option<MouseMessage>,

    /// Defines how many points a user needs to scroll horizontally to make a "tick" on a mouse/touchpad/touchscreen
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 30.0)))]
    pub horizontal_scroll_threshold: Option<f32>,
    /// Command to send on scrolling left (every tick)
    pub on_scroll_left: Option<MouseMessage>,
    /// Command to send on scrolling right (every tick)
    pub on_scroll_right: Option<MouseMessage>,
}

impl MouseConfig {
    pub fn has_command(&self) -> bool {
        [
            &self.on_primary_double_click,
            &self.on_secondary_click,
            &self.on_middle_click,
            &self.on_extra1_click,
            &self.on_extra2_click,
            &self.on_scroll_up,
            &self.on_scroll_down,
            &self.on_scroll_left,
            &self.on_scroll_right,
        ]
        .iter()
        .any(|opt| matches!(opt, Some(MouseMessage::Command(_))))
    }
}

impl MouseMessage {
    pub fn execute(&self, mouse_follows_focus: bool) {
        match self {
            MouseMessage::Komorebi(config) => {
                let mut messages = Vec::new();

                if config.focus_monitor_at_cursor.unwrap_or(true) {
                    messages.push(SocketMessage::FocusMonitorAtCursor);
                }

                if config.ignore_mouse_follows_focus.unwrap_or(true) {
                    messages.push(SocketMessage::MouseFollowsFocus(false));
                    messages.push(config.message.clone());
                    messages.push(SocketMessage::MouseFollowsFocus(mouse_follows_focus));
                } else {
                    messages.push(config.message.clone());
                }

                tracing::debug!("Sending messages: {messages:?}");

                if komorebi_client::send_batch(messages).is_err() {
                    tracing::error!("could not send commands");
                }
            }
            MouseMessage::Command(cmd) => {
                tracing::debug!("Executing command: {}", cmd);

                let cmd_no_env = cmd.replace_env();

                if exec_powershell(cmd_no_env.to_str().expect("Invalid command")).is_err() {
                    tracing::error!("Failed to execute '{}'", cmd);
                }
            }
        };
    }
}

impl KomobarConfig {
    pub fn read(path: &PathBuf) -> color_eyre::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = match path.extension().unwrap().to_string_lossy().as_str() {
            "json" => serde_json::from_str(&content)?,
            _ => panic!("unsupported format"),
        };

        komorebi_bar_core::widgets::widget::warn_unsupported(
            value
                .left_widgets
                .iter()
                .chain(value.center_widgets.iter().flatten())
                .chain(value.right_widgets.iter()),
        );

        if value.frame.is_none() {
            value.frame = Some(FrameConfig {
                inner_margin: Position {
                    x: DEFAULT_PADDING,
                    y: DEFAULT_PADDING,
                },
            });
        }

        Ok(value)
    }
}

impl RenderExt for &KomobarConfig {
    fn new_renderconfig(
        &self,
        ctx: &Context,
        background_color: Color32,
        icon_scale: Option<f32>,
    ) -> RenderConfig {
        let text_font_id = ctx
            .style()
            .text_styles
            .get(&TextStyle::Body)
            .cloned()
            .unwrap_or_else(FontId::default);

        let mut icon_font_id = text_font_id.clone();
        icon_font_id.size *= icon_scale.unwrap_or(1.4).clamp(1.0, 2.0);

        let monitor_idx = match &self.monitor {
            Some(MonitorConfigOrIndex::MonitorConfig(monitor_config)) => monitor_config.index,
            Some(MonitorConfigOrIndex::Index(idx)) => *idx,
            None => 0,
        };

        // check if any of the alignments have a komorebi widget with the workspace set to show all icons
        let show_all_icons =
            KomobarConfig::show_all_icons_on_komorebi_workspace(&self.left_widgets)
                || self
                    .center_widgets
                    .as_ref()
                    .is_some_and(|list| KomobarConfig::show_all_icons_on_komorebi_workspace(list))
                || KomobarConfig::show_all_icons_on_komorebi_workspace(&self.right_widgets);

        RenderConfig {
            monitor_idx,
            spacing: self.widget_spacing.unwrap_or(10.0),
            grouping: self.grouping.unwrap_or(Grouping::None),
            background_color,
            alignment: None,
            more_inner_margin: false,
            applied_on_widget: false,
            text_font_id,
            icon_font_id,
            show_all_icons,
            auto_select_fill: NonZeroU32::new(AUTO_SELECT_FILL_COLOUR.load(Ordering::SeqCst))
                .map(|c| Colour::Rgb(Rgb::from(c.get())).into()),
            auto_select_text: NonZeroU32::new(AUTO_SELECT_TEXT_COLOUR.load(Ordering::SeqCst))
                .map(|c| Colour::Rgb(Rgb::from(c.get())).into()),
        }
    }
}

pub use komorebi_bar_core::config::types::*;
pub use komorebi_themes::KomobarTheme;
