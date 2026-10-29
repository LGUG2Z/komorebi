//! The viewer's preferences, kept between runs: which sessions were opened and how it was left.
//!
//! The preferences are one small JSON file, `viewer-prefs.json`, in [`directory`] (komorebi's data
//! directory). They are conveniences and never worth refusing to start over, so a missing or
//! damaged file reads as the defaults and a failure to write one is ignored. Nothing here needs a
//! window. [`ViewerApp`](super::ViewerApp) loads them when it is made and saves them when a
//! session is opened and when the state overlay is switched.

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

const FILE: &str = "viewer-prefs.json";
const RECENT: usize = 12;

/// The viewer's preferences, kept between runs.
///
/// A field missing from the file takes its default, so a file written by an older build with fewer
/// fields still reads.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::viewer::prefs::Prefs;
/// use std::path::Path;
///
/// let mut prefs = Prefs::default();
/// assert!(prefs.overlay && prefs.compact);
///
/// prefs.opened(Path::new("/recordings/a"));
/// prefs.opened(Path::new("/recordings/b"));
/// prefs.opened(Path::new("/recordings/a"));
/// assert_eq!(prefs.recent, ["/recordings/a", "/recordings/b"]);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// The sessions opened, as the text of their paths, most recent first. Each is listed once and
    /// at most the twelve most recent are kept.
    pub recent: Vec<String>,
    /// Whether komorebi's state is drawn over the video. The default is `true`.
    pub overlay: bool,
    /// Whether the window, when started with no session to show, is the small control window: the
    /// Record tab alone, always on top. Switched with the Compact / Expand button. The default is
    /// `true`, so a file from a build that did not have it keeps the small window.
    pub compact: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            overlay: true,
            compact: true,
        }
    }
}

impl Prefs {
    /// Returns the preferences kept in `dir`, or the defaults if there are none or they are damaged.
    ///
    /// They are only conveniences, never worth refusing to start over, so this has no error: a
    /// missing directory, a file that cannot be read and a file that is not valid JSON of this
    /// shape all give [`Prefs::default`].
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::viewer::prefs::Prefs;
    ///
    /// let name = format!("komorebi-recorder-doc-load-{}", std::process::id());
    /// let dir = std::env::temp_dir().join(name);
    /// std::fs::create_dir_all(&dir)?;
    ///
    /// // nothing saved yet
    /// assert_eq!(Prefs::load(&dir), Prefs::default());
    ///
    /// // damaged
    /// std::fs::write(dir.join("viewer-prefs.json"), "not json {")?;
    /// assert_eq!(Prefs::load(&dir), Prefs::default());
    ///
    /// // an older file with fewer fields still reads
    /// std::fs::write(dir.join("viewer-prefs.json"), r#"{"overlay": false}"#)?;
    /// assert!(!Prefs::load(&dir).overlay);
    /// # std::fs::remove_dir_all(&dir)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the preferences to `viewer-prefs.json` in `dir`, creating `dir` if it is missing.
    ///
    /// A failure to create the directory or to write the file is ignored, for the reason given at
    /// [`Prefs::load`]: the preferences are not worth interrupting anyone for.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::viewer::prefs::Prefs;
    /// use std::path::Path;
    ///
    /// let name = format!("komorebi-recorder-doc-save-{}", std::process::id());
    /// let dir = std::env::temp_dir().join(name);
    ///
    /// let mut prefs = Prefs { overlay: false, ..Prefs::default() };
    /// prefs.opened(Path::new("/recordings/a"));
    /// prefs.save(&dir);
    /// assert_eq!(Prefs::load(&dir), prefs);
    /// # std::fs::remove_dir_all(&dir)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn save(&self, dir: &Path) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join(FILE), text);
        }
    }

    /// Notes that `session` was opened, moving it to the front of [`Prefs::recent`].
    ///
    /// It is listed once, however often it is opened, and the list keeps the twelve most recent.
    /// The path is stored as it is given, not made absolute or resolved.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::viewer::prefs::Prefs;
    /// use std::path::Path;
    ///
    /// let mut prefs = Prefs::default();
    /// for index in 0..30 {
    ///     prefs.opened(Path::new(&format!("/recordings/s{index}")));
    /// }
    /// assert_eq!(prefs.recent.len(), 12);
    /// assert_eq!(prefs.recent[0], "/recordings/s29");
    /// ```
    pub fn opened(&mut self, session: &Path) {
        let text = session.display().to_string();
        self.recent.retain(|recent| *recent != text);
        self.recent.insert(0, text);
        self.recent.truncate(RECENT);
    }

    /// Returns the recent sessions that still exist, most recent first.
    ///
    /// A recent session still exists if its directory has a session manifest
    /// ([`MANIFEST_FILE`](crate::session::MANIFEST_FILE)). Nothing else about it is checked, so a
    /// session that exists may still fail to open.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::session::MANIFEST_FILE;
    /// use komorebi_recorder::viewer::prefs::Prefs;
    /// use std::path::Path;
    ///
    /// let name = format!("komorebi-recorder-doc-existing-{}", std::process::id());
    /// let session = std::env::temp_dir().join(name);
    /// std::fs::create_dir_all(&session)?;
    /// std::fs::write(session.join(MANIFEST_FILE), "{}")?;
    ///
    /// let mut prefs = Prefs::default();
    /// prefs.opened(&session);
    /// prefs.opened(Path::new("/gone/forever"));
    /// assert_eq!(prefs.existing(), [session.clone()]);
    /// # std::fs::remove_dir_all(&session)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn existing(&self) -> Vec<PathBuf> {
        self.recent
            .iter()
            .map(PathBuf::from)
            .filter(|path| path.join(crate::session::MANIFEST_FILE).is_file())
            .collect()
    }
}

/// The environment variable that replaces [`directory`]. It exists so that the window can be tried
/// out, or tested end to end, without reading or writing the user's real preferences; it is not
/// meant for anything else.
pub const DIRECTORY_VARIABLE: &str = "KOMOREBI_RECORDER_PREFS_DIR";

/// Returns where preferences live: `$KOMOREBI_RECORDER_PREFS_DIR` if it is set and not empty
/// ([`DIRECTORY_VARIABLE`]), else komorebi's data directory, `<local data dir>/komorebi` (see
/// [`komorebi_link::data_dir`](crate::komorebi_link::data_dir)).
///
/// It depends on the machine, so it is not shown with an example: the viewer passes it to
/// [`Prefs::load`] and [`Prefs::save`].
#[must_use]
pub fn directory() -> PathBuf {
    match std::env::var_os(DIRECTORY_VARIABLE) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => crate::komorebi_link::data_dir().to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-prefs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn preferences_round_trip_and_default_when_missing() {
        let dir = temp("round");
        assert_eq!(Prefs::load(&dir), Prefs::default());

        let mut prefs = Prefs {
            overlay: false,
            ..Prefs::default()
        };
        prefs.opened(Path::new("/a"));
        prefs.save(&dir);
        assert_eq!(Prefs::load(&dir), prefs);
    }

    #[test]
    fn damaged_preferences_are_ignored_not_fatal() {
        let dir = temp("damaged");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(FILE), "not json {").expect("write");
        assert_eq!(Prefs::load(&dir), Prefs::default());

        // an older file with fewer fields still reads
        std::fs::write(dir.join(FILE), r#"{"overlay": false}"#).expect("write");
        assert!(!Prefs::load(&dir).overlay);
    }

    #[test]
    fn the_most_recent_session_is_first_and_each_is_listed_once() {
        let mut prefs = Prefs::default();
        prefs.opened(Path::new("/a"));
        prefs.opened(Path::new("/b"));
        prefs.opened(Path::new("/a"));
        assert_eq!(prefs.recent, ["/a", "/b"]);

        for index in 0..30 {
            prefs.opened(Path::new(&format!("/s{index}")));
        }
        assert_eq!(prefs.recent.len(), RECENT);
        assert_eq!(prefs.recent[0], "/s29");
    }

    #[test]
    fn only_sessions_that_still_exist_are_offered() {
        let (session, _) = crate::testkit::SessionBuilder::new("prefs-existing").build();
        let mut prefs = Prefs::default();
        prefs.opened(&session.dir);
        prefs.opened(Path::new("/gone/forever"));

        assert_eq!(prefs.existing(), [session.dir]);
    }
}
