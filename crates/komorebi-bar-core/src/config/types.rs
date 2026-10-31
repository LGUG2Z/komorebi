use eframe::egui::Pos2;
use eframe::egui::Vec2;
use serde::Deserialize;
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Position
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

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Label prefix
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
/// Display format
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
    ($(#[$type_meta:meta])* $existing_enum:ident, $new_enum:ident, { $($(#[$meta:meta])* $variant:ident),* $(,)? }) => {
        #[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        $(#[$type_meta])*
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

extend_enum!(
    /// Workspaces display format
    DisplayFormat, WorkspacesDisplayFormat, {
    /// Show all icons only
    AllIcons,
    /// Show both all icons and text
    AllIconsAndText,
    /// Show all icons and text for the selected element, and all icons on the rest
    AllIconsAndTextOnSelected,
});

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Media widget display format
pub enum MediaDisplayFormat {
    /// Show only the media info icon
    Icon,
    /// Show only the media info text (artist - title)
    Text,
    /// Show both icon and text
    IconAndText,
    /// Show only the control buttons (previous, play/pause, next)
    ControlsOnly,
    /// Show icon with control buttons
    IconAndControls,
    /// Show text with control buttons
    TextAndControls,
    /// Show icon, text, and control buttons
    Full,
}

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
