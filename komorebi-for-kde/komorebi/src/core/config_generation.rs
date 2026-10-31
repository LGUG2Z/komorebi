//! Rules that match an application by how it identifies itself.
//!
//! The types are defined in `komorebi-config`, which `komorebi.json` is read with.

pub use komorebi_config::matching::IdWithIdentifier;
pub use komorebi_config::matching::IdWithIdentifierAndComment;
pub use komorebi_config::matching::MatchingRule;
pub use komorebi_config::matching::MatchingStrategy;
pub use komorebi_config::matching::WorkspaceMatchingRule;
