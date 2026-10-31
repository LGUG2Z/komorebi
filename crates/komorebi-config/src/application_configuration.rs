//! The entries that the Windows `fetch-app-specific-configuration`
//! generator reads, and the options that turn into rules.
//!
//! The generator itself (it reads YAML) stays in the Windows daemon; the data types are here so
//! that the conversion into `ApplicationSpecificConfiguration` can be a `From` impl next to it.

use clap::ValueEnum;
use komorebi_protocol::types::ApplicationIdentifier;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::matching::IdWithIdentifier;
use crate::matching::MatchingRule;
use crate::matching::MatchingStrategy;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ApplicationOptions {
    ObjectNameChange,
    Layered,
    TrayAndMultiWindow,
    Force,
    BorderOverflow,
}

impl ApplicationOptions {
    #[must_use]
    pub fn raw_cfgen(&self, kind: &ApplicationIdentifier, id: &str) -> String {
        match self {
            ApplicationOptions::ObjectNameChange => {
                format!("komorebic.exe identify-object-name-change-application {kind} \"{id}\"",)
            }
            ApplicationOptions::Layered => {
                format!("komorebic.exe identify-layered-application {kind} \"{id}\"",)
            }
            ApplicationOptions::TrayAndMultiWindow => {
                format!("komorebic.exe identify-tray-application {kind} \"{id}\"",)
            }
            ApplicationOptions::Force => {
                format!("komorebic.exe manage-rule {kind} \"{id}\"")
            }
            ApplicationOptions::BorderOverflow => {
                unreachable!("deprecated");
            }
        }
    }

    #[must_use]
    pub fn cfgen(&self, kind: &ApplicationIdentifier, id: &str) -> String {
        format!(
            "RunWait('{}', , \"Hide\")",
            ApplicationOptions::raw_cfgen(self, kind, id)
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ApplicationConfiguration {
    pub name: String,
    pub identifier: IdWithIdentifier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ApplicationOptions>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "float_identifiers")]
    pub ignore_identifiers: Option<Vec<MatchingRule>>,
}

impl ApplicationConfiguration {
    pub fn populate_default_matching_strategies(&mut self) {
        if self.identifier.matching_strategy.is_none() {
            match self.identifier.kind {
                ApplicationIdentifier::Exe | ApplicationIdentifier::Path => {
                    self.identifier.matching_strategy = Option::from(MatchingStrategy::Equals);
                }
                ApplicationIdentifier::Class | ApplicationIdentifier::Title => {}
            }
        }
    }
}
