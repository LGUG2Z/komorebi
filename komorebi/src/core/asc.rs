use crate::config_generation::ApplicationConfiguration;
use crate::config_generation::ApplicationOptions;
use crate::config_generation::MatchingRule;
use color_eyre::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ApplicationSpecificConfiguration(pub BTreeMap<String, AscApplicationRulesOrSchema>);

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
    /// Rules to ignore specific windows from the transparency feature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency_ignore: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which minimize to the tray or have multiple windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_and_multi_window: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which have the `WS_EX_LAYERED` Extended Window Style
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layered: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which send the `EVENT_OBJECT_NAMECHANGE` event on launch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_change: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which are slow to send initial event notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_application: Option<Vec<MatchingRule>>,
}

impl From<Vec<ApplicationConfiguration>> for ApplicationSpecificConfiguration {
    fn from(value: Vec<ApplicationConfiguration>) -> Self {
        let mut map = BTreeMap::new();

        for entry in &value {
            let key = entry.name.clone();
            let mut rules = AscApplicationRules {
                ignore: None,
                manage: None,
                floating: None,
                transparency_ignore: None,
                tray_and_multi_window: None,
                layered: None,
                object_name_change: None,
                slow_application: None,
            };

            rules.ignore = entry.ignore_identifiers.clone();

            if let Some(options) = &entry.options {
                for opt in options {
                    match opt {
                        ApplicationOptions::ObjectNameChange => {
                            rules.object_name_change =
                                Some(vec![MatchingRule::Simple(entry.identifier.clone())]);
                        }
                        ApplicationOptions::Layered => {
                            rules.layered =
                                Some(vec![MatchingRule::Simple(entry.identifier.clone())]);
                        }
                        ApplicationOptions::TrayAndMultiWindow => {
                            rules.tray_and_multi_window =
                                Some(vec![MatchingRule::Simple(entry.identifier.clone())]);
                        }
                        ApplicationOptions::Force => {
                            rules.manage =
                                Some(vec![MatchingRule::Simple(entry.identifier.clone())]);
                        }
                        ApplicationOptions::BorderOverflow => {}
                    }
                }
            }

            if rules.ignore.is_some()
                || rules.manage.is_some()
                || rules.floating.is_some()
                || rules.transparency_ignore.is_some()
                || rules.tray_and_multi_window.is_some()
                || rules.layered.is_some()
                || rules.object_name_change.is_some()
                || rules.slow_application.is_some()
            {
                map.insert(key, AscApplicationRulesOrSchema::AscApplicationRules(rules));
            }
        }

        Self(map)
    }
}
