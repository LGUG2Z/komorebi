//! Keeping file access inside the session directory.
//!
//! Session files name other session files (a timeline entry's `file`, an annotation's log
//! reference), and sessions are handed between people and agents. Nothing read from a session may
//! therefore be allowed to point outside it: absolute paths, `..`, and symlinks that lead out are
//! all refused.
//!
//! [`SessionPath`] is the checked session directory, and [`SessionPath::resolve`] is the one way the
//! analysis turns a name from inside a session into a path. [`is_plain_relative`] is the lexical
//! check it starts with. Failures are errors that carry an exit kind, `Usage` for a name that
//! points out and `NotFound` for a directory that is not a session, so a command that hits one
//! fails with the matching exit code (see the `envelope` module).
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof | Escape hatch |
//! |---|---|---|---|
//! | A path from [`SessionPath::resolve`] is a plain relative path under the session root | runtime error: `Usage` | `escapes_are_refused` | none |
//! | An existing path whose real location is outside the session is refused, symlinks included | runtime error: `Usage` | `a_symlink_that_leads_out_is_refused` (Unix only) | none |
//! | A path that does not exist yet is checked lexically only | not guaranteed: the symlink check needs the path to exist | none | none |
//! | A [`SessionPath`] is a directory with a manifest | runtime error: `NotFound`, in [`SessionPath::new`] | `a_directory_without_a_manifest_is_not_a_session` | none |
//! | A file whose name is read from the contents of a session is opened through [`SessionPath::resolve`] | convention only: files with a fixed name, such as `events.jsonl`, are opened without it | none | none |

use super::envelope;
use crate::session::MANIFEST_FILE;
use color_eyre::eyre;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// Returns whether `relative` is a non-empty relative path made only of ordinary names: no `..`,
/// no `.`, no root or drive prefix.
///
/// This looks at the text only. It does not look at the file system, so it cannot see a symlink;
/// [`SessionPath::resolve`] does that.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::guard::is_plain_relative;
///
/// assert!(is_plain_relative("events.jsonl"));
/// assert!(is_plain_relative("oslog/apps/x.jsonl"));
///
/// assert!(!is_plain_relative(""));
/// assert!(!is_plain_relative("."));
/// assert!(!is_plain_relative("../x"));
/// assert!(!is_plain_relative("a/../../x"));
/// assert!(!is_plain_relative("/etc/passwd"));
/// ```
#[must_use]
pub fn is_plain_relative(relative: &str) -> bool {
    !relative.is_empty()
        && Path::new(relative)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// A session directory known to be one, with its real (symlink-free) location.
///
/// It is constructed only by [`SessionPath::new`], which checks that the directory has a manifest,
/// so a value of this type is guaranteed to name a directory that held one when it was made. Whether
/// the directory still does is not checked again. Files are reached through
/// [`SessionPath::resolve`], which refuses any name that points outside it.
#[derive(Debug, Clone)]
pub struct SessionPath {
    root: PathBuf,
}

impl SessionPath {
    /// Returns the checked session directory `dir`, with symlinks resolved.
    ///
    /// The directory is made canonical with `dunce`, which, unlike [`Path::canonicalize`], does
    /// not give a `\\?\` path on Windows, so paths shown to a reader or an agent can be copied.
    ///
    /// # Errors
    ///
    /// Both cases are a `NotFound` error:
    ///
    /// - `dir` cannot be opened (it does not exist or cannot be made canonical):
    ///   `cannot open <dir>: <error>`;
    /// - `dir` has no manifest file (`session.json`), so it is not a recording session:
    ///   `<dir> is not a recording session (no session.json)`.
    ///
    /// # Example
    ///
    /// Reading a session needs a session directory on disk, so this example is only compiled.
    /// It is covered by `a_directory_without_a_manifest_is_not_a_session` and
    /// `plain_relative_paths_resolve_inside_the_session` in this module's tests.
    ///
    /// ```rust,no_run
    /// use komorebi_recorder::analysis::guard::SessionPath;
    /// use std::path::Path;
    ///
    /// let session = SessionPath::new(Path::new("/path/to/session"))?;
    /// println!("{}", session.root().display());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(dir: &Path) -> eyre::Result<Self> {
        // not `Path::canonicalize`: on Windows that returns a `\\?\` path, which would then show up
        // in every path this tool prints for an agent to copy
        let root = dunce::canonicalize(dir).map_err(|error| {
            envelope::not_found(format!("cannot open {}: {error}", dir.display()))
        })?;

        if !root.join(MANIFEST_FILE).is_file() {
            return Err(envelope::not_found(format!(
                "{} is not a recording session (no {MANIFEST_FILE})",
                dir.display()
            )));
        }

        Ok(Self { root })
    }

    /// Returns the session directory: absolute, with symlinks resolved.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the path of `relative` inside the session. It must be a plain relative path (see
    /// [`is_plain_relative`]) and, if it exists, must really be inside the session once symlinks
    /// are followed.
    ///
    /// The returned path is `root().join(relative)`: it is not canonicalized, and the file it names
    /// is not opened or required to exist.
    ///
    /// # Errors
    ///
    /// Both cases are a `Usage` error:
    ///
    /// - `relative` is not a plain relative path (it is empty, absolute, or has a `..`, a `.` or
    ///   a drive prefix): `'<relative>' is not a path inside the session`;
    /// - `relative` exists and, with every symlink followed, is outside the session:
    ///   `'<relative>' leads outside the session`.
    ///
    /// A path that does not exist passes the second check, because there is nothing to follow. If
    /// one of its parent directories is a symlink that leads out, the path is still returned.
    ///
    /// # Example
    ///
    /// Covered by `escapes_are_refused` and `a_symlink_that_leads_out_is_refused` in this module's
    /// tests; the example needs a session directory on disk, so it is only compiled.
    ///
    /// ```rust,no_run
    /// use komorebi_recorder::analysis::guard::SessionPath;
    /// use std::path::Path;
    ///
    /// let session = SessionPath::new(Path::new("/path/to/session"))?;
    /// let events = session.resolve("events.jsonl")?;
    /// assert!(events.starts_with(session.root()));
    /// assert!(session.resolve("../elsewhere").is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn resolve(&self, relative: &str) -> eyre::Result<PathBuf> {
        let path = Path::new(relative);
        if !is_plain_relative(relative) {
            return Err(envelope::usage(format!(
                "'{relative}' is not a path inside the session"
            )));
        }

        let joined = self.root.join(path);
        if let Ok(real) = dunce::canonicalize(&joined)
            && !real.starts_with(&self.root)
        {
            return Err(envelope::usage(format!(
                "'{relative}' leads outside the session"
            )));
        }

        Ok(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-guard-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn a_directory_without_a_manifest_is_not_a_session() {
        let dir = temp("empty");
        let error = SessionPath::new(&dir).expect_err("not a session");
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::NotFound);
        let missing = SessionPath::new(&dir.join("nope")).expect_err("missing");
        assert_eq!(envelope::exit_kind(&missing), envelope::ExitKind::NotFound);
    }

    #[test]
    fn plain_relative_paths_resolve_inside_the_session() {
        let dir = temp("plain");
        std::fs::write(dir.join(MANIFEST_FILE), "{}").expect("manifest");
        let session = SessionPath::new(&dir).expect("session");

        let path = session.resolve("oslog/apps/x.jsonl").expect("resolve");
        assert!(path.starts_with(session.root()));
        assert!(path.ends_with("oslog/apps/x.jsonl"));
    }

    #[test]
    fn escapes_are_refused() {
        let dir = temp("escape");
        std::fs::write(dir.join(MANIFEST_FILE), "{}").expect("manifest");
        let session = SessionPath::new(&dir).expect("session");

        for bad in ["../x", "a/../../x", "/etc/passwd", "", "."] {
            assert!(session.resolve(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_leads_out_is_refused() {
        let outside = temp("outside");
        std::fs::write(outside.join("secret.txt"), "x").expect("secret");
        let dir = temp("symlink");
        std::fs::write(dir.join(MANIFEST_FILE), "{}").expect("manifest");
        std::os::unix::fs::symlink(&outside, dir.join("link")).expect("symlink");
        let session = SessionPath::new(&dir).expect("session");

        assert!(session.resolve("link/secret.txt").is_err());
        assert!(session.resolve("link").is_err());
        // a symlink that stays inside is fine
        std::fs::create_dir_all(dir.join("real")).expect("real");
        std::os::unix::fs::symlink(dir.join("real"), dir.join("alias")).expect("alias");
        assert!(session.resolve("alias").is_ok());
    }
}
