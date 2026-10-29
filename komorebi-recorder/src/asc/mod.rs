//! Rules for komorebi-application-specific-configuration, written from what a recording shows.
//!
//! [komorebi-application-specific-configuration](https://github.com/LGUG2Z/komorebi-application-specific-configuration)
//! (the ASC) is where the rules that make applications "just work" with komorebi are kept for
//! everyone. A recording that shows a ghost tile ([`crate::analysis::ghost`]) is evidence for one,
//! and this module turns it into a pull request:
//!
//! - [`rule`] is the rule, as narrow as it has to be to leave the application's other windows
//!   alone, and puts it into the repository's rule file without reformatting it;
//! - [`submit`] clones the repository into a temporary directory, commits the rule on a branch with
//!   a message that says how it was found and why, and opens a pull request with the credentials
//!   the user has already set up, and, if that cannot be done, says what is left for the user to do.
//!
//! Each komorebi has a file of its own in the repository, and a rule goes in the file of the
//! komorebi the recording is of, not of the machine it is looked at on: a Mac can look at a
//! recording of komorebi for Windows and the rule is still for Windows. See [`Platform`].

pub mod rule;
pub mod submit;

use crate::analysis::flavor::Flavor;
use schemars::JsonSchema;
use serde::Serialize;

/// Which komorebi a rule is for, and so which of the repository's files it goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    /// komorebi for Windows: `applications.json`. Programs are executable file names, and a window
    /// has a class.
    Windows,
    /// komorebi for Mac: `applications.mac.json`. Programs are application names, and a window has
    /// no class.
    Macos,
    /// komorebi for KDE. Its rules are in `komorebic/applications.json` of its own repository, and
    /// nothing here writes or proposes one yet. Programs are application ids, as are classes.
    Kde,
}

impl Platform {
    /// Returns the platform of the komorebi a recording is of.
    #[must_use]
    pub fn of(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Windows => Self::Windows,
            Flavor::Macos => Self::Macos,
            Flavor::Kde => Self::Kde,
        }
    }

    /// Returns the name of the rule file of this platform in the repository.
    #[must_use]
    pub fn file(self) -> &'static str {
        match self {
            Self::Windows => "applications.json",
            Self::Macos => "applications.mac.json",
            Self::Kde => "komorebic/applications.json",
        }
    }

    /// Returns what the platform's komorebi is called.
    #[must_use]
    pub fn komorebi(self) -> &'static str {
        match self {
            Self::Windows => "komorebi for Windows",
            Self::Macos => "komorebi for Mac",
            Self::Kde => "komorebi for KDE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_komorebi_has_its_own_file() {
        assert_eq!(Platform::of(Flavor::Windows).file(), "applications.json");
        assert_eq!(Platform::of(Flavor::Macos).file(), "applications.mac.json");
    }
}
