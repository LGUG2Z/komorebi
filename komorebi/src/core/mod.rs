#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]
#![allow(deprecated)] // allow deprecated variants like HidingBehaviour::Hide to be used in derive macros


use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;


// Re-export everything from komorebi-layouts
pub use komorebi_layouts::Arrangement;
pub use komorebi_layouts::Axis;
pub use komorebi_layouts::Column;
pub use komorebi_layouts::ColumnSplit;
pub use komorebi_layouts::ColumnSplitWithCapacity;
pub use komorebi_layouts::ColumnWidth;
pub use komorebi_layouts::CustomLayout;
pub use komorebi_layouts::CycleDirection;
pub use komorebi_layouts::DEFAULT_RATIO;
pub use komorebi_layouts::DEFAULT_SECONDARY_RATIO;
pub use komorebi_layouts::DefaultLayout;
pub use komorebi_layouts::Direction;
pub use komorebi_layouts::GridLayoutOptions;
pub use komorebi_layouts::Layout;
pub use komorebi_layouts::LayoutDefaultEntry;
pub use komorebi_layouts::LayoutOptions;
pub use komorebi_layouts::MAX_RATIO;
pub use komorebi_layouts::MAX_RATIOS;
pub use komorebi_layouts::MIN_RATIO;
pub use komorebi_layouts::OperationDirection;
pub use komorebi_layouts::PanDirection;
pub use komorebi_layouts::PanGesture;
pub use komorebi_layouts::Rect;
pub use komorebi_layouts::ScrollingLayoutOptions;
pub use komorebi_layouts::Sizing;
pub use komorebi_layouts::scrolling::ScrollingCentering;
pub use komorebi_layouts::validate_ratios;

// Local modules and exports
pub use animation::AnimationStyle;
pub use komorebi_config::behaviour::CrossBoundaryBehaviour;
pub use komorebi_config::behaviour::FloatingLayerBehaviour;
pub use komorebi_config::behaviour::Placement;
pub use komorebi_config::behaviour::WindowContainerBehaviour;
pub use komorebi_config::behaviour::WindowHandlingBehaviour;
pub use komorebi_protocol::types::ApplicationIdentifier;
pub use komorebi_protocol::types::BorderImplementation;
pub use komorebi_protocol::types::BorderStyle;
pub use komorebi_protocol::types::FocusFollowsMouseImplementation;
pub use komorebi_protocol::types::HidingBehaviour;
pub use komorebi_protocol::types::MonocleFocusBehaviour;
pub use komorebi_protocol::types::MoveBehaviour;
pub use komorebi_protocol::types::OperationBehaviour;
pub use komorebi_protocol::types::StackbarLabel;
pub use komorebi_protocol::types::StackbarMode;
pub use komorebi_protocol::types::StateQuery;
pub use komorebi_protocol::types::SubscribeOptions;
pub use komorebi_protocol::types::WindowKind;
pub use pathext::PathExt;
pub use pathext::ResolvedPathBuf;
pub use pathext::replace_env_in_path;
pub use pathext::resolve_option_hashmap_usize_path;

pub mod animation;
pub mod asc;
pub mod config_generation;
pub use komorebi_protocol::pathext;

pub use komorebi_protocol::message::SocketMessage;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window management behaviour
pub struct WindowManagementBehaviour {
    /// The current [`WindowContainerBehaviour`] to be used
    pub current_behaviour: WindowContainerBehaviour,
    /// Override of `current_behaviour` to open new windows as floating windows
    /// that can be later toggled to tiled, when false it will default to
    /// `current_behaviour` again.
    pub float_override: bool,
    /// Determines if a new window should be spawned floating when on the floating layer and the
    /// floating layer behaviour is set to float. This value is always calculated when checking for
    /// the management behaviour on a specific workspace.
    pub floating_layer_override: bool,
    /// The floating layer behaviour to be used if the float override is being used
    pub floating_layer_behaviour: FloatingLayerBehaviour,
    /// The `Placement` to be used when toggling a window to float
    pub toggle_float_placement: Placement,
    /// The `Placement` to be used when spawning a window on the floating layer with the
    /// `FloatingLayerBehaviour` set to `FloatingLayerBehaviour::Float`
    pub floating_layer_placement: Placement,
    /// The `Placement` to be used when spawning a window with float override active
    pub float_override_placement: Placement,
    /// The `Placement` to be used when spawning a window that matches a `floating_applications` rule
    pub float_rule_placement: Placement,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    PartialEq,
    Eq,
    Hash,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Placement strategy for new windows in a workspace
pub enum WindowPlacement {
    /// Place the new window at the primary (largest) container position
    Primary,
    /// Place the new window at the secondary container position
    Secondary,
    /// Place the new window before the currently focused container
    BeforeFocused,
    /// Place the new window after the currently focused container (default behaviour)
    #[default]
    AfterFocused,
    /// Place the new window at the end of the container list
    Last,
}

/// A target position for window placement, used as a key in `InitialWindowPlacementRules::Rules`.
///
/// Can be either a `WindowPlacement` variant name (e.g. `"Primary"`, `"Last"`)
/// or a 1-based container index (e.g. `"1"`, `"3"`).
///
/// NOTE: Integer indices are 1-based in the config for user-friendliness.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlacementTarget {
    /// A named placement strategy
    Placement(WindowPlacement),
    /// A 1-based container index
    Index(usize),
}

impl std::fmt::Display for PlacementTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlacementTarget::Placement(p) => write!(f, "{p}"),
            PlacementTarget::Index(i) => write!(f, "{i}"),
        }
    }
}

impl std::str::FromStr for PlacementTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try parsing as a WindowPlacement variant first
        if let Ok(placement) = <WindowPlacement as std::str::FromStr>::from_str(s) {
            return Ok(PlacementTarget::Placement(placement));
        }
        // Then try parsing as a 1-based index
        if let Ok(idx) = s.parse::<usize>() {
            return Ok(PlacementTarget::Index(idx));
        }
        Err(format!(
            "'{s}' is not a valid WindowPlacement variant or integer"
        ))
    }
}

impl PartialOrd for PlacementTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// Written by hand because the (de)serialization is: a `WindowPlacement` name or a container index,
// as a string or as an integer, never the externally tagged object a derive would describe
#[cfg(feature = "schemars")]
impl schemars::JsonSchema for PlacementTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PlacementTarget".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A target position for window placement, used as a key in `InitialWindowPlacementRules::Rules`.

Can be either a `WindowPlacement` variant name (e.g. `\"Primary\"`, `\"Last\"`)
or a 1-based container index (e.g. `\"1\"`, `\"3\"`, or the integer `3`).

NOTE: Integer indices are 1-based in the config for user-friendliness.",
            "anyOf": [
                generator.subschema_for::<WindowPlacement>(),
                {
                    "description": "A 1-based container index as a string",
                    "type": "string",
                    "pattern": "^[0-9]+$"
                },
                {
                    "description": "A 1-based container index",
                    "type": "integer",
                    "minimum": 0
                }
            ]
        })
    }
}

impl Ord for PlacementTarget {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_string().cmp(&other.to_string())
    }
}

impl Serialize for PlacementTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PlacementTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PlacementTargetVisitor;

        impl serde::de::Visitor<'_> for PlacementTargetVisitor {
            type Value = PlacementTarget;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a WindowPlacement name or a non-negative container index")
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
                s.parse().map_err(E::custom)
            }

            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<Self::Value, E> {
                usize::try_from(n)
                    .map(PlacementTarget::Index)
                    .map_err(E::custom)
            }

            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<Self::Value, E> {
                usize::try_from(n)
                    .map(PlacementTarget::Index)
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PlacementTargetVisitor)
    }
}

/// Configuration for initial window placement rules on a workspace.
///
/// This can be specified in two forms in the JSON config:
/// - A placement target (string or integer) — applies the same placement to all windows.
///   Strings are `WindowPlacement` variant names (e.g. `"Primary"`, `"AfterFocused"`),
///   integers are 1-based container indices.
/// - A map of placement targets to matching rules — keys can be `WindowPlacement` variant names
///   (e.g. `"Primary"`, `"Secondary"`) or 1-based container indices (e.g. `"1"`, `"3"`).
///   Rules are evaluated in key order; the first matching rule determines placement.
///
/// NOTE: Container indices in the config are 1-based for user-friendliness.
/// They are converted to 0-based internally during resolution.
///
/// NOTE: This feature currently only applies when `WindowContainerBehaviour::Create` is active.
/// Future versions may support toggling this for `Append` mode as well.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum InitialWindowPlacementRules {
    /// A single placement target applied to all new windows (string or integer in config)
    Target(PlacementTarget),
    /// A map of placement targets to matching rules.
    /// Keys can be `WindowPlacement` variant names (e.g. `"Primary"`) or 1-based container indices (e.g. `"1"`).
    /// Values can be:
    /// - A single `IdWithIdentifier` object (simple rule)
    /// - An array containing objects and/or arrays:
    ///   - Each object in the array is an independent simple rule (OR logic between entries)
    ///   - Each inner array is a composite rule where all conditions must match (AND logic)
    ///   - The outer array entries are evaluated with OR logic
    ///
    /// Rules are evaluated in key order; the first matching rule determines placement.
    Rules(std::collections::BTreeMap<PlacementTarget, PlacementMatchingRules>),
}

/// Matching rules for a placement target.
///
/// Can be specified in JSON as:
/// - A single `IdWithIdentifier` object — one simple rule
/// - An array of `MatchingRule`s — multiple rules with OR logic between them
///   (each element can be a simple rule object or a composite rule array with AND logic)
///
/// Examples:
/// ```json
/// // Single rule
/// { "kind": "Exe", "id": "chrome.exe", "matching_strategy": "Equals" }
///
/// // Multiple rules (OR): chrome OR teams
/// [
///   { "kind": "Exe", "id": "chrome.exe", "matching_strategy": "Equals" },
///   { "kind": "Title", "id": "Microsoft Teams", "matching_strategy": "Equals" }
/// ]
///
/// // Mixed: chrome OR (code.exe AND title contains "workspace")
/// [
///   { "kind": "Exe", "id": "chrome.exe", "matching_strategy": "Equals" },
///   [
///     { "kind": "Exe", "id": "code.exe", "matching_strategy": "Equals" },
///     { "kind": "Title", "id": "workspace", "matching_strategy": "Contains" }
///   ]
/// ]
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PlacementMatchingRules {
    /// A single simple matching rule
    Single(config_generation::IdWithIdentifier),
    /// Multiple matching rules evaluated with OR logic.
    /// Each entry is a `MatchingRule`: either a simple rule (object) or composite rule (array, AND logic).
    Many(Vec<config_generation::MatchingRule>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn deserializes() {
        // Set a variable for testing
        unsafe {
            std::env::set_var("VAR", "VALUE");
        }

        let json = r#"{"type":"WorkspaceLayoutCustomRule","content":[0,0,0,"/path/%VAR%/d"]}"#;
        let message: SocketMessage = serde_json::from_str(json).unwrap();

        let SocketMessage::WorkspaceLayoutCustomRule(
            _workspace_index,
            _workspace_number,
            _monitor_index,
            path,
        ) = message
        else {
            panic!("Expected WorkspaceLayoutCustomRule");
        };

        assert_eq!(path, PathBuf::from("/path/VALUE/d"));
    }

    #[test]
    fn placement_target_accepts_a_number_or_a_string() {
        let parse = |json: &str| serde_json::from_str::<InitialWindowPlacementRules>(json);

        let index = InitialWindowPlacementRules::Target(PlacementTarget::Index(2));
        assert_eq!(parse("2").unwrap(), index);
        assert_eq!(parse(r#""2""#).unwrap(), index);
        assert_eq!(
            parse(r#""Primary""#).unwrap(),
            InitialWindowPlacementRules::Target(PlacementTarget::Placement(
                WindowPlacement::Primary
            ))
        );

        assert!(parse("-1").is_err());
        assert!(parse("1.5").is_err());
        assert!(parse("true").is_err());

        // the keys of the map form are always strings
        let Ok(InitialWindowPlacementRules::Rules(rules)) =
            parse(r#"{"2": {"kind": "Exe", "id": "a.exe"}, "Primary": []}"#)
        else {
            panic!("expected the map form");
        };
        assert!(rules.contains_key(&PlacementTarget::Index(2)));
        assert!(rules.contains_key(&PlacementTarget::Placement(WindowPlacement::Primary)));
    }
}
