//! The application specific configuration file (`app_specific_configuration_path`): rules, by
//! application, for the windows komorebi ignores, manages or floats.

use std::collections::BTreeMap;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;

use color_eyre::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::application_configuration::ApplicationConfiguration;
use crate::application_configuration::ApplicationOptions;
use crate::matching::MatchingRule;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ApplicationSpecificConfiguration(pub BTreeMap<String, AscApplicationRulesOrSchema>);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(clippy::large_enum_variant)] // the rules are the common case; the enum is a map value
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
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
    /// Rules to ignore specific windows from the transparency feature
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub transparency_ignore: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which minimize to the tray or have multiple windows
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub tray_and_multi_window: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which have the `WS_EX_LAYERED` Extended Window Style
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub layered: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which send the `EVENT_OBJECT_NAMECHANGE` event on launch
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub object_name_change: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which are slow to send initial event notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub slow_application: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which are title-less - only accepts Simple Exe rules
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["kde", "macos"])))]
    pub titleless: Option<Vec<MatchingRule>>,
    /// Rules to identify applications which are title-less - only accepts Simple Exe rules
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["kde", "macos"])))]
    pub tabbed: Option<Vec<MatchingRule>>,
}

impl From<Vec<ApplicationConfiguration>> for ApplicationSpecificConfiguration {
    fn from(value: Vec<ApplicationConfiguration>) -> Self {
        let mut map = BTreeMap::new();

        for entry in &value {
            let key = entry.name.clone();
            let mut rules = AscApplicationRules {
                ignore: entry.ignore_identifiers.clone(),
                ..AscApplicationRules::default()
            };

            if let Some(options) = &entry.options {
                for opt in options {
                    let rule = Some(vec![MatchingRule::Simple(entry.identifier.clone())]);
                    match opt {
                        ApplicationOptions::ObjectNameChange => rules.object_name_change = rule,
                        ApplicationOptions::Layered => rules.layered = rule,
                        ApplicationOptions::TrayAndMultiWindow => {
                            rules.tray_and_multi_window = rule
                        }
                        ApplicationOptions::Force => rules.manage = rule,
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
