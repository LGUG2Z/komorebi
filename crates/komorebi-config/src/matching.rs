//! Rules that match an application by how it identifies itself.
//!
//! A [`MatchingRule`] is the form every list of rules in the configuration takes: a single
//! [`IdWithIdentifier`], or a list of them that must all match.

use serde::Deserialize;
use serde::Serialize;
use strum::Display;

use komorebi_protocol::types::ApplicationIdentifier;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_that_is_an_object_is_simple() {
        let rule: MatchingRule = serde_json::from_str(
            r#"{ "kind": "Exe", "id": "foot", "matching_strategy": "Equals" }"#,
        )
        .expect("parses");

        assert_eq!(
            rule,
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: "foot".to_string(),
                matching_strategy: Some(MatchingStrategy::Equals),
            })
        );
    }

    #[test]
    fn a_rule_that_is_a_list_is_composite() {
        let rule: MatchingRule = serde_json::from_str(
            r#"[{ "kind": "Exe", "id": "foot" }, { "kind": "Title", "id": "scratch", "matching_strategy": "StartsWith" }]"#,
        )
        .expect("parses");

        let MatchingRule::Composite(parts) = rule else {
            panic!("a list of objects is a composite rule");
        };
        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[1].matching_strategy,
            Some(MatchingStrategy::StartsWith)
        );
    }

    #[test]
    fn the_matching_strategy_is_left_out_when_there_is_none() {
        let rule = IdWithIdentifier {
            kind: ApplicationIdentifier::Class,
            id: "firefox".to_string(),
            matching_strategy: None,
        };

        let json = serde_json::to_string(&rule).expect("serializes");

        assert_eq!(json, r#"{"kind":"Class","id":"firefox"}"#);
    }

    #[test]
    fn a_commented_rule_becomes_a_plain_one() {
        let commented: IdWithIdentifierAndComment = serde_json::from_str(
            r#"{ "kind": "Path", "id": "/usr/bin/kcalc", "comment": "a calculator" }"#,
        )
        .expect("parses");

        let plain = IdWithIdentifier::from(commented);

        assert_eq!(plain.kind, ApplicationIdentifier::Path);
        assert_eq!(plain.id, "/usr/bin/kcalc");
        assert_eq!(plain.matching_strategy, None);
    }
}
