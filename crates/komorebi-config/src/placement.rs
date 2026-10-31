//! Where a new window goes in a workspace: the placement strategies, the targets that name a
//! place and the rules that pick one.

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::matching;

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
    /// A name or a number: `"Primary"`, `"3"` and `3` (the documentation of the setting shows a
    /// number, and a JSON object's keys can only be text)
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Number(usize),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
            Raw::Number(index) => Ok(PlacementTarget::Index(index)),
        }
    }
}

/// The schema is written by hand, since the one `schemars` derives for the enum (an object with a
/// `Placement` or an `Index` key) is not what [`Serialize`] and [`Deserialize`] read and write: a
/// name, a number, or text that is a number.
#[cfg(feature = "schemars")]
impl schemars::JsonSchema for PlacementTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("PlacementTarget")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A place in the list of containers: the name of a placement strategy, or a 1-based container index, as a number or as text (`3` and `\"3\"` are the same place; the keys of a JSON object can only be text).",
            "anyOf": [
                generator.subschema_for::<WindowPlacement>(),
                {
                    "description": "A 1-based container index",
                    "type": "integer",
                    "format": "uint",
                    "minimum": 0
                },
                {
                    "description": "A 1-based container index, as text",
                    "type": "string",
                    "pattern": "^[0-9]+$"
                }
            ]
        })
    }
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
    Single(matching::IdWithIdentifier),
    /// Multiple matching rules evaluated with OR logic.
    /// Each entry is a `MatchingRule`: either a simple rule (object) or composite rule (array, AND logic).
    Many(Vec<matching::MatchingRule>),
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
