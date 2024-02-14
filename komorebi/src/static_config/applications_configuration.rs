/*
  This file is the seed for a future refactoring to unify all applications identifiers
  in one instead of have a lot of separated identifier object for each configuration
*/

use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::Arc,
};

use color_eyre::eyre::Result;
use getset::Getters;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use regex::Regex;

use komorebi_core::{
    config_generation::{
        ApplicationConfiguration, ApplicationOptions, IdWithIdentifier, MatchingStrategy,
    },
    ApplicationIdentifier,
};

use crate::{window::Window, REGEX_IDENTIFIERS};

lazy_static! {
    pub static ref SETTINGS_BY_APP: Arc<Mutex<AppsConfigurations>> =
        Arc::new(Mutex::new(AppsConfigurations::default()));
}

#[derive(Getters, Clone)]
pub struct AppIdentifier {
    id: String,
    kind: ApplicationIdentifier,
    matching_strategy: MatchingStrategy,
}

impl From<IdWithIdentifier> for AppIdentifier {
    fn from(value: IdWithIdentifier) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            matching_strategy: value
                .matching_strategy
                .map_or(MatchingStrategy::Legacy, |strategy| strategy),
        }
    }
}

impl AppIdentifier {
    pub fn cache_regex(&mut self) -> Result<()> {
        if matches!(self.matching_strategy, MatchingStrategy::Regex) {
            let re = Regex::new(&self.id)?;
            REGEX_IDENTIFIERS.lock().insert(self.id.clone(), re);
        }
        Ok(())
    }

    pub fn validate(&self, title: &str, class: &str, exe: &str) -> bool {
        match self.matching_strategy {
            MatchingStrategy::Legacy => match self.kind {
                ApplicationIdentifier::Title => {
                    title.starts_with(&self.id) || title.ends_with(&self.id)
                }
                ApplicationIdentifier::Class => {
                    class.starts_with(&self.id) || class.ends_with(&self.id)
                }
                ApplicationIdentifier::Exe => exe.eq(&self.id),
            },
            MatchingStrategy::Equals => match self.kind {
                ApplicationIdentifier::Title => title.eq(&self.id),
                ApplicationIdentifier::Class => class.eq(&self.id),
                ApplicationIdentifier::Exe => exe.eq(&self.id),
            },
            MatchingStrategy::StartsWith => match self.kind {
                ApplicationIdentifier::Title => title.starts_with(&self.id),
                ApplicationIdentifier::Class => class.starts_with(&self.id),
                ApplicationIdentifier::Exe => exe.starts_with(&self.id),
            },
            MatchingStrategy::EndsWith => match self.kind {
                ApplicationIdentifier::Title => title.ends_with(&self.id),
                ApplicationIdentifier::Class => class.ends_with(&self.id),
                ApplicationIdentifier::Exe => exe.ends_with(&self.id),
            },
            MatchingStrategy::Contains => match self.kind {
                ApplicationIdentifier::Title => title.contains(&self.id),
                ApplicationIdentifier::Class => class.contains(&self.id),
                ApplicationIdentifier::Exe => exe.contains(&self.id),
            },
            MatchingStrategy::Regex => {
                let regex_identifiers = REGEX_IDENTIFIERS.lock();
                if let Some(re) = regex_identifiers.get(&self.id) {
                    return match self.kind {
                        ApplicationIdentifier::Title => re.is_match(title),
                        ApplicationIdentifier::Class => re.is_match(class),
                        ApplicationIdentifier::Exe => re.is_match(exe),
                    };
                }
                false
            }
        }
    }
}

#[derive(Getters, Clone)]
pub struct AppConfig {
    #[allow(dead_code)] // remove on use after refactor
    #[getset(get = "pub")]
    name: String,
    #[getset(get = "pub")]
    category: Option<String>,
    #[getset(get = "pub")]
    identifier: AppIdentifier,
    #[allow(dead_code)] // remove on use after refactor
    #[getset(get = "pub")]
    options: Vec<ApplicationOptions>,
}

impl From<ApplicationConfiguration> for AppConfig {
    fn from(value: ApplicationConfiguration) -> Self {
        Self {
            name: value.name,
            category: value.category,
            identifier: value.identifier.into(),
            options: value.options.map_or_else(|| Vec::new(), |options| options),
        }
    }
}

impl AppConfig {
    pub fn match_window(&self, window: &Window) -> bool {
        if let (Ok(title), Ok(exe), Ok(class)) = (window.title(), window.exe(), window.class()) {
            return self.identifier.validate(&title, &class, &exe);
        }
        false
    }
}

pub struct AppsConfigurations {
    apps: VecDeque<AppConfig>,
    cache: HashMap<isize, Option<usize>>,
}

impl Default for AppsConfigurations {
    fn default() -> Self {
        Self {
            apps: VecDeque::new(),
            cache: HashMap::new(),
        }
    }
}

impl AppsConfigurations {
    pub fn add(&mut self, mut app: AppConfig) -> Result<()> {
        app.identifier.cache_regex()?;
        self.apps.push_front(app);
        self.cache.clear();
        Ok(())
    }

    pub fn get_by_window(&mut self, window: &Window) -> Option<&AppConfig> {
        match self.cache.entry(window.hwnd) {
            Entry::Occupied(entry) => entry.get().and_then(|index| self.apps.get(index)),
            Entry::Vacant(entry) => {
                for (i, app) in self.apps.iter().enumerate() {
                    if app.match_window(window) {
                        entry.insert(Some(i));
                        return Option::from(app);
                    }
                }
                entry.insert(None);
                None
            }
        }
    }
}
