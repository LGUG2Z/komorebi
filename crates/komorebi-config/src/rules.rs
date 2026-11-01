//! Which of a list of rules a window matches.
//!
//! The evaluation every daemon shares: [`should_act`] returns the last rule of a list that matches
//! a window ([`RuleSubject`]: its title, executable, classes and path), a composite rule matching
//! when all of its conditions do, and [`should_act_individual`] decides one condition by its
//! matching strategy. A `Regex` id is looked up in a cache the daemon compiled when it loaded its
//! rules (an id with no compiled regex matches nothing); the cache is passed in so the lock that
//! guards it stays the daemon's. The one difference between the platforms is [`RuleQuirks`].

use std::collections::HashMap;

use regex::Regex;

use komorebi_protocol::types::ApplicationIdentifier;

use crate::matching::IdWithIdentifier;
use crate::matching::MatchingRule;
use crate::matching::MatchingStrategy;

/// What a window says about itself, as the rules see it
#[derive(Clone, Copy, Debug)]
pub struct RuleSubject<'a> {
    pub title: &'a str,
    /// The executable name; on Windows it carries the `.exe`, elsewhere it does not
    pub exe_name: &'a str,
    /// The window's classes; a `Class` condition holds when it holds for any of them. Windows has
    /// exactly one
    pub classes: &'a [&'a str],
    pub path: &'a str,
}

/// The differences between the platforms in how an id is read
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleQuirks {
    /// Remove every `.exe` from a rule id before comparing (also from a regex id, and so from the
    /// key looked up in the regex cache): a rule written the Windows way names the same program
    /// where executables have no suffix
    pub strip_exe_from_ids: bool,
}

impl RuleQuirks {
    /// Windows: the id is compared as written
    pub const WINDOWS: Self = Self {
        strip_exe_from_ids: false,
    };
    /// KDE and macOS: a `.exe` in the id is ignored
    pub const UNIX: Self = Self {
        strip_exe_from_ids: true,
    };
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
/// Returns the last of `identifiers` that matches the window, if one does: a simple rule matches
/// when its one condition does ([`should_act_individual`]), and a composite rule when all of its
/// conditions do
pub fn should_act(
    subject: &RuleSubject,
    quirks: RuleQuirks,
    identifiers: &[MatchingRule],
    regex_identifiers: &HashMap<String, Regex>,
) -> Option<MatchingRule> {
    let mut matching_rule = None;
    for rule in identifiers {
        match rule {
            MatchingRule::Simple(identifier) => {
                if should_act_individual(subject, quirks, identifier, regex_identifiers) {
                    matching_rule = Some(rule.clone());
                };
            }
            MatchingRule::Composite(identifiers) => {
                let mut composite_results = vec![];
                for identifier in identifiers {
                    composite_results.push(should_act_individual(
                        subject,
                        quirks,
                        identifier,
                        regex_identifiers,
                    ));
                }

                if composite_results.iter().all(|&x| x) {
                    matching_rule = Some(rule.clone());
                }
            }
        }
    }

    matching_rule
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
/// Returns true if and only if the condition `identifier` holds for the window
///
/// The condition compares the title, the executable, any of the classes or the path with its id,
/// by its matching strategy. Without one, or with `Legacy`, a title or a class matches when it
/// starts or ends with the id, and an executable or a path when it is the id. A `Regex` id is
/// looked up in `regex_identifiers`, and an id with no compiled regex there matches nothing.
pub fn should_act_individual(
    subject: &RuleSubject,
    quirks: RuleQuirks,
    identifier: &IdWithIdentifier,
    regex_identifiers: &HashMap<String, Regex>,
) -> bool {
    let RuleSubject {
        title,
        exe_name,
        classes,
        path,
    } = *subject;
    let mut should_act = false;

    let stripped;
    let identifier = if quirks.strip_exe_from_ids {
        let mut copy = identifier.clone();
        copy.id = copy.id.replace(".exe", "");
        stripped = copy;
        &stripped
    } else {
        identifier
    };

    match identifier.matching_strategy {
        None | Some(MatchingStrategy::Legacy) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.starts_with(&identifier.id) || title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.starts_with(&identifier.id) || class.ends_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.eq(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::Equals) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.eq(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.eq(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotEqual) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.eq(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.eq(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::StartsWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.starts_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotStartWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.starts_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::EndsWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.ends_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotEndWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.ends_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::Contains) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.contains(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.contains(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotContain) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.contains(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.contains(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::Regex) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if let Some(re) = regex_identifiers.get(&identifier.id)
                    && re.is_match(title)
                {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                if let Some(re) = regex_identifiers.get(&identifier.id) {
                    for class in classes {
                        if re.is_match(class) {
                            should_act = true;
                        }
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if let Some(re) = regex_identifiers.get(&identifier.id)
                    && re.is_match(exe_name)
                {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if let Some(re) = regex_identifiers.get(&identifier.id)
                    && re.is_match(path)
                {
                    should_act = true;
                }
            }
        },
    }

    should_act
}
