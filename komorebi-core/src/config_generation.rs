use clap::ValueEnum;
use color_eyre::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::ApplicationIdentifier;

#[derive(Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ApplicationOptions {
    ObjectNameChange,
    Layered,
    BorderOverflow,
    TrayAndMultiWindow,
    Force,
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
            ApplicationOptions::BorderOverflow => {
                format!("komorebic.exe identify-border-overflow-application {kind} \"{id}\"",)
            }
            ApplicationOptions::TrayAndMultiWindow => {
                format!("komorebic.exe identify-tray-application {kind} \"{id}\"",)
            }
            ApplicationOptions::Force => {
                format!("komorebic.exe manage-rule {kind} \"{id}\"")
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IdWithIdentifier {
    pub kind: ApplicationIdentifier,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matching_strategy: Option<MatchingStrategy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum MatchingStrategy {
    Legacy,
    Equals,
    StartsWith,
    EndsWith,
    Contains,
    Regex,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ApplicationConfiguration {
    pub name: String,
    pub category: Option<String>,
    pub identifier: IdWithIdentifier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ApplicationOptions>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_identifiers: Option<Vec<IdWithIdentifierAndComment>>,
}

impl ApplicationConfiguration {
    pub fn populate_default_matching_strategies(&mut self) {
        if self.identifier.matching_strategy.is_none() {
            match self.identifier.kind {
                ApplicationIdentifier::Exe => {
                    self.identifier.matching_strategy = Option::from(MatchingStrategy::Equals);
                }
                ApplicationIdentifier::Class | ApplicationIdentifier::Title => {}
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ApplicationConfigurationGenerator;

impl ApplicationConfigurationGenerator {
    pub fn load(content: &str) -> Result<Vec<ApplicationConfiguration>> {
        Ok(serde_yaml::from_str(content)?)
    }

    pub fn format(content: &str) -> Result<String> {
        let mut cfgen = Self::load(content)?;
        for cfg in &mut cfgen {
            cfg.populate_default_matching_strategies();
        }

        cfgen.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(serde_yaml::to_string(&cfgen)?)
    }

    fn merge(base_content: &str, override_content: &str) -> Result<Vec<ApplicationConfiguration>> {
        let base_cfgen = Self::load(base_content)?;
        let override_cfgen = Self::load(override_content)?;

        let mut final_cfgen = base_cfgen.clone();

        for entry in override_cfgen {
            let mut replace_idx = None;
            for (idx, base_entry) in base_cfgen.iter().enumerate() {
                if base_entry.name == entry.name {
                    replace_idx = Option::from(idx);
                }
            }

            match replace_idx {
                None => final_cfgen.push(entry),
                Some(idx) => final_cfgen[idx] = entry,
            }
        }

        Ok(final_cfgen)
    }

    pub fn generate_pwsh(
        base_content: &str,
        override_content: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut cfgen = if let Some(override_content) = override_content {
            Self::merge(base_content, override_content)?
        } else {
            Self::load(base_content)?
        };

        cfgen.sort_by(|a, b| a.name.cmp(&b.name));

        let mut lines = vec![String::from("# Generated by komorebic.exe"), String::new()];

        let mut float_rules = vec![];

        for app in cfgen {
            lines.push(format!("# {}", app.name));
            if let Some(options) = app.options {
                for opt in options {
                    if matches!(opt, ApplicationOptions::TrayAndMultiWindow) {
                        lines.push(String::from("# If you have disabled minimize/close to tray for this application, you can delete/comment out the next line"));
                    }

                    lines.push(opt.raw_cfgen(&app.identifier.kind, &app.identifier.id));
                }
            }

            if let Some(float_identifiers) = app.float_identifiers {
                for float in float_identifiers {
                    let float_rule =
                        format!("komorebic.exe float-rule {} \"{}\"", float.kind, float.id);

                    // Don't want to send duped signals especially as configs get larger
                    if !float_rules.contains(&float_rule) {
                        float_rules.push(float_rule.clone());

                        if let Some(comment) = float.comment {
                            lines.push(format!("# {comment}"));
                        };

                        lines.push(float_rule);
                    }
                }
            }

            lines.push(String::new());
        }

        Ok(lines)
    }

    pub fn generate_ahk(base_content: &str, override_content: Option<&str>) -> Result<Vec<String>> {
        let mut cfgen = if let Some(override_content) = override_content {
            Self::merge(base_content, override_content)?
        } else {
            Self::load(base_content)?
        };

        cfgen.sort_by(|a, b| a.name.cmp(&b.name));

        let mut lines = vec![String::from("; Generated by komorebic.exe"), String::new()];

        let mut float_rules = vec![];

        for app in cfgen {
            lines.push(format!("; {}", app.name));
            if let Some(options) = app.options {
                for opt in options {
                    if matches!(opt, ApplicationOptions::TrayAndMultiWindow) {
                        lines.push(String::from("; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line"));
                    }

                    lines.push(opt.cfgen(&app.identifier.kind, &app.identifier.id));
                }
            }

            if let Some(float_identifiers) = app.float_identifiers {
                for float in float_identifiers {
                    let float_rule = format!(
                        "RunWait('komorebic.exe float-rule {} \"{}\"', , \"Hide\")",
                        float.kind, float.id
                    );

                    // Don't want to send duped signals especially as configs get larger
                    if !float_rules.contains(&float_rule) {
                        float_rules.push(float_rule.clone());

                        if let Some(comment) = float.comment {
                            lines.push(format!("; {comment}"));
                        };

                        lines.push(float_rule);
                    }
                }
            }

            lines.push(String::new());
        }

        Ok(lines)
    }
}
