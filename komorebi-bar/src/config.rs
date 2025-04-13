use crate::render::Grouping;
use crate::widgets::widget::WidgetConfig;
use crate::DEFAULT_PADDING;
use eframe::egui::Pos2;
use eframe::egui::TextBuffer;
use eframe::egui::Vec2;
use komorebi_client::KomorebiTheme;
use komorebi_client::Rect;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// The `komorebi.bar.json` configuration file reference for `v0.1.37`
pub struct KomobarConfig {
    /// Bar height (default: 50)
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
    pub monitor: MonitorConfigOrIndex,
    /// Font family
    pub font_family: Option<String>,
    /// Font size (default: 12.5)
    pub font_size: Option<f32>,
    /// Scale of the icons relative to the font_size [[1.0-2.0]]. (default: 1.4)
    pub icon_scale: Option<f32>,
    /// Max label width before text truncation (default: 400.0)
    pub max_label_width: Option<f32>,
    /// Theme
    pub theme: Option<KomobarTheme>,
    /// Alpha value for the color transparency [[0-255]] (default: 200)
    pub transparency_alpha: Option<u8>,
    /// Spacing between widgets (default: 10.0)
    pub widget_spacing: Option<f32>,
    /// Visual grouping for widgets
    pub grouping: Option<Grouping>,
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
pub struct FrameConfig {
    /// Margin inside the painted frame
    pub inner_margin: Position,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum MonitorConfigOrIndex {
    /// The monitor index where you want the bar to show
    Index(usize),
    /// The full monitor options with the index and an optional work_area_offset
    MonitorConfig(MonitorConfig),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
pub enum SpacingKind {
    All(f32),
    Individual(IndividualSpacingConfig),
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
pub struct GroupedSpacingConfig {
    pub vertical: Option<GroupedSpacingOptions>,
    pub horizontal: Option<GroupedSpacingOptions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum GroupedSpacingOptions {
    Symmetrical(f32),
    Split(f32, f32),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct IndividualSpacingConfig {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
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

impl KomobarConfig {
    pub fn read(path: &PathBuf) -> color_eyre::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = match path.extension().unwrap().to_string_lossy().as_str() {
            "json" => serde_json::from_str(&content)?,
            _ => panic!("unsupported format"),
        };

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

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "palette")]
pub enum KomobarTheme {
    /// A theme from catppuccin-egui
    Catppuccin {
        /// Name of the Catppuccin theme (theme previews: https://github.com/catppuccin/catppuccin)
        name: komorebi_themes::Catppuccin,
        accent: Option<komorebi_themes::CatppuccinValue>,
        auto_select_fill: Option<komorebi_themes::CatppuccinValue>,
        auto_select_text: Option<komorebi_themes::CatppuccinValue>,
    },
    /// A theme from base16-egui-themes
    Base16 {
        /// Name of the Base16 theme (theme previews: https://tinted-theming.github.io/tinted-gallery/)
        name: komorebi_themes::Base16,
        accent: Option<komorebi_themes::Base16Value>,
        auto_select_fill: Option<komorebi_themes::Base16Value>,
        auto_select_text: Option<komorebi_themes::Base16Value>,
    },
    /// A custom Base16 theme
    Custom {
        /// Colours of the custom Base16 theme palette
        colours: Box<komorebi_themes::Base16ColourPalette>,
        accent: Option<komorebi_themes::Base16Value>,
        auto_select_fill: Option<komorebi_themes::Base16Value>,
        auto_select_text: Option<komorebi_themes::Base16Value>,
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
                auto_select_fill: None,
                auto_select_text: None,
            },
            KomorebiTheme::Base16 {
                name, bar_accent, ..
            } => Self::Base16 {
                name,
                accent: bar_accent,
                auto_select_fill: None,
                auto_select_text: None,
            },
            KomorebiTheme::Custom {
                colours,
                bar_accent,
                ..
            } => Self::Custom {
                colours,
                accent: bar_accent,
                auto_select_fill: None,
                auto_select_text: None,
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DisplayFormat {
    /// Show only icon
    Icon,
    /// Show only text
    Text,
    /// Show an icon and text for the selected element, and text on the rest
    TextAndIconOnSelected,
    /// Show both icon and text
    IconAndText,
    /// Show an icon and text for the selected element, and icons on the rest
    IconAndTextOnSelected,
}

macro_rules! extend_enum {
    ($existing_enum:ident, $new_enum:ident, { $($(#[$meta:meta])* $variant:ident),* $(,)? }) => {
        #[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        pub enum $new_enum {
            // Add new variants
            $(
                $(#[$meta])*
                $variant,
            )*
            // Include a variant that wraps the existing enum and flatten it when deserializing
            #[serde(untagged)]
            Existing($existing_enum),
        }

        // Implement From for the existing enum
        impl From<$existing_enum> for $new_enum {
            fn from(value: $existing_enum) -> Self {
                $new_enum::Existing(value)
            }
        }
    };
}

extend_enum!(DisplayFormat, WorkspacesDisplayFormat, {
    /// Show all icons only
    AllIcons,
    /// Show both all icons and text
    AllIconsAndText,
    /// Show all icons and text for the selected element, and all icons on the rest
    AllIconsAndTextOnSelected,
});

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde::Serialize;
    use serde_json::json;

    #[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum OriginalDisplayFormat {
        /// Show None Of The Things
        NoneOfTheThings,
    }

    extend_enum!(OriginalDisplayFormat, ExtendedDisplayFormat, {
        /// Show Some Of The Things
        SomeOfTheThings,
    });

    #[derive(serde::Deserialize)]
    struct ExampleConfig {
        #[allow(unused)]
        format: ExtendedDisplayFormat,
    }

    #[test]
    pub fn extend_new_variant() {
        let raw = json!({
            "format": "SomeOfTheThings",
        })
        .to_string();

        assert!(serde_json::from_str::<ExampleConfig>(&raw).is_ok())
    }

    #[test]
    pub fn extend_existing_variant() {
        let raw = json!({
            "format": "NoneOfTheThings",
        })
        .to_string();

        assert!(serde_json::from_str::<ExampleConfig>(&raw).is_ok())
    }

    #[test]
    pub fn extend_invalid_variant() {
        let raw = json!({
            "format": "ALLOFTHETHINGS",
        })
        .to_string();

        assert!(serde_json::from_str::<ExampleConfig>(&raw).is_err())
    }
}
