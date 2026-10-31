use crate::core::config_generation::MatchingRule;
use color_eyre::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ApplicationSpecificConfiguration(pub BTreeMap<String, AscApplicationRulesOrSchema>);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum AscApplicationRulesOrSchema {
    AscApplicationRules(AscApplicationRules),
    Schema(String),
}

impl Deref for ApplicationSpecificConfiguration {
    type Target = BTreeMap<String, AscApplicationRulesOrSchema>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ApplicationSpecificConfiguration {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ApplicationSpecificConfiguration {
    pub fn load(pathbuf: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(pathbuf)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn format(pathbuf: &PathBuf) -> Result<String> {
        Ok(serde_json::to_string_pretty(&Self::load(pathbuf)?)?)
    }
}

/// Rules that determine how an application is handled
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AscApplicationRules {
    /// Rules to ignore specific windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<MatchingRule>>,
    /// Rules to forcibly manage specific windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage: Option<Vec<MatchingRule>>,
    /// Rules to manage specific windows as floating windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which are title-less - only accepts Simple Exe rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titleless: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which are title-less - only accepts Simple Exe rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabbed: Option<Vec<MatchingRule>>,
}
