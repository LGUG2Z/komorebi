use serde::Deserialize;
use serde::Serialize;
use strum::Display;

use super::ApplicationIdentifier;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Rule for matching applications
pub enum MatchingRule {
    /// Simple matching rule which must evaluate to true
    Simple(IdWithIdentifier),
    /// Composite matching rule where all conditions must evaluate to true
    Composite(Vec<IdWithIdentifier>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Rule for assigning applications to a workspace
pub struct WorkspaceMatchingRule {
    /// Target monitor index
    pub monitor_index: usize,
    /// Target workspace index
    pub workspace_index: usize,
    /// Matching rule for the application
    pub matching_rule: MatchingRule,
    /// Whether to apply the rule only when the application is initially launched
    pub initial_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Rule for matching applications
pub struct IdWithIdentifier {
    /// Kind of identifier to target
    pub kind: ApplicationIdentifier,
    /// Target identifier
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Matching strategy to use
    pub matching_strategy: Option<MatchingStrategy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Strategy for matching identifiers
pub enum MatchingStrategy {
    /// Should not be used, only kept for backward compatibility
    Legacy,
    /// Equals
    Equals,
    /// Starts With
    StartsWith,
    /// Ends With
    EndsWith,
    /// Contains
    Contains,
    /// Regex
    Regex,
    /// Does not end with
    DoesNotEndWith,
    /// Does not start with
    DoesNotStartWith,
    /// Does not equal
    DoesNotEqual,
    /// Does not contain
    DoesNotContain,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct IdWithIdentifierAndComment {
    pub kind: ApplicationIdentifier,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matching_strategy: Option<MatchingStrategy>,
}

impl From<IdWithIdentifierAndComment> for IdWithIdentifier {
    fn from(value: IdWithIdentifierAndComment) -> Self {
        Self {
            kind: value.kind,
            id: value.id.clone(),
            matching_strategy: value.matching_strategy,
        }
    }
}
