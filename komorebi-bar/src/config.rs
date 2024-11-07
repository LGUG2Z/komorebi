use crate::widget::WidgetConfig;
use eframe::egui::Pos2;
use eframe::egui::TextBuffer;
use eframe::egui::Vec2;
use komorebi_client::KomorebiTheme;
use komorebi_client::Rect;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// The `komorebi.bar.json` configuration file reference for `v0.1.31`
pub struct KomobarConfig {
    /// Bar positioning options
    #[serde(alias = "viewport")]
    pub position: Option<PositionConfig>,
    /// Frame options (see: https://docs.rs/egui/latest/egui/containers/frame/struct.Frame.html)
    pub frame: Option<FrameConfig>,
    /// Monitor options
    pub monitor: MonitorConfig,
    /// Font family
    pub font_family: Option<String>,
    /// Font size (default: 12.5)
    pub font_size: Option<f32>,
    /// Max label width before text truncation (default: 400.0)
    pub max_label_width: Option<f32>,
    /// Theme
    pub theme: Option<KomobarTheme>,
    /// Visual grouping for widgets
    pub group: Option<Group>,
    /// Left side widgets (ordered left-to-right)
    pub left_widgets: Vec<WidgetConfig>,
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
            println!("\nYour bar configuration file contains some options that have been renamed or deprecated:\n");
            for (canonical, aliases) in map {
                for alias in aliases {
                    if raw.contains(alias) {
                        println!(r#""{alias}" is now "{canonical}""#);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PositionConfig {
    /// The desired starting position of the bar (0,0 = top left of the screen)
    #[serde(alias = "position")]
    pub start: Option<Position>,
    /// The desired size of the bar from the starting position (usually monitor width x desired height)
    #[serde(alias = "inner_size")]
    pub end: Option<Position>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrameConfig {
    /// Margin inside the painted frame
    pub inner_margin: Position,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MonitorConfig {
    /// Komorebi monitor index of the monitor on which to render the bar
    pub index: usize,
    /// Automatically apply a work area offset for this monitor to accommodate the bar
    pub work_area_offset: Option<Rect>,
}

impl KomobarConfig {
    pub fn read(path: &PathBuf) -> color_eyre::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = match path.extension().unwrap().to_string_lossy().as_str() {
            "json" => serde_json::from_str(&content)?,
            _ => panic!("unsupported format"),
        };

        if value.frame.is_none() {
            value.frame = Some(FrameConfig {
                inner_margin: Position { x: 10.0, y: 10.0 },
            });
        }

        Ok(value)
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Position {
    /// X coordinate
    pub x: f32,
    /// Y coordinate
    pub y: f32,
}

impl From<Position> for Vec2 {
    fn from(value: Position) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<Position> for Pos2 {
    fn from(value: Position) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "palette")]
pub enum KomobarTheme {
    /// A theme from catppuccin-egui
    Catppuccin {
        /// Name of the Catppuccin theme (theme previews: https://github.com/catppuccin/catppuccin)
        name: komorebi_themes::Catppuccin,
        accent: Option<komorebi_themes::CatppuccinValue>,
    },
    /// A theme from base16-egui-themes
    Base16 {
        /// Name of the Base16 theme (theme previews: https://tinted-theming.github.io/base16-gallery)
        name: komorebi_themes::Base16,
        accent: Option<komorebi_themes::Base16Value>,
    },
}

impl From<KomorebiTheme> for KomobarTheme {
    fn from(value: KomorebiTheme) -> Self {
        match value {
            KomorebiTheme::Catppuccin {
                name, bar_accent, ..
            } => Self::Catppuccin {
                name,
                accent: bar_accent,
            },
            KomorebiTheme::Base16 {
                name, bar_accent, ..
            } => Self::Base16 {
                name,
                accent: bar_accent,
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum LabelPrefix {
    /// Show no prefix
    None,
    /// Show an icon
    Icon,
    /// Show text
    Text,
    /// Show an icon and text
    IconAndText,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Group {
    None
}