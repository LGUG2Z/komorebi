//! Which komorebi was running, so that what the log says can be tied to the right source.
//!
//! komorebi's log names no version and a user's installed binary is often not the checkout that
//! is being debugged. Mapping a log line to a line of source is only honest for the commit that
//! produced it, so the recorder asks the running binary what it is: `komorebi --version` prints
//!
//! ```text
//! komorebi 0.1.0
//! branch:master
//! commit_hash:0cc4ca7f67d79da0d59d603dde2f6b4445fec85b
//! ```
//!
//! [`parse`] reads that text and [`query`] runs the binary. The result is a [`KomorebiVersion`],
//! which the manifest (`session.json`) stores in `komorebi.versions`, one for each komorebi process
//! seen during the recording, in order. The analysis compares the recorded commit with the source
//! it indexes (`analysis::catalog`) to say whether a log line can be mapped to a line of source.
//! The same code runs for both flavors of komorebi, on both hosts.

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// A running komorebi's identity: which binary, and so which source, produced its log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct KomorebiVersion {
    /// The process this was read from. `None` when the version was parsed from text alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    /// The version number from the first line of the output, e.g. `0.1.0`.
    pub version: String,
    /// The git branch the binary was built from (the `branch:` line). `None` if the output has no
    /// such line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The git commit the binary was built from (the `commit_hash:` line), as the binary prints
    /// it: a full or an abbreviated hash. `None` if the output has no such line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

impl KomorebiVersion {
    /// Returns whether this binary was built from `commit` (full or abbreviated), or `None` when
    /// that cannot be said.
    ///
    /// The two hashes are compared over the length of the shorter. The answer is `None` if this
    /// version has no commit, or if the shorter hash has fewer than 7 characters, which is too
    /// short to tell commits apart.
    ///
    /// # Panics
    ///
    /// Panics if the shorter length (in bytes) is not a character boundary of the longer string.
    /// A git hash is hexadecimal, so this cannot happen for one.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::sources::komorebi_version::parse;
    ///
    /// let version = parse("komorebi 0.1.0\nbranch:master\ncommit_hash:0cc4ca7f67d79da0d59d603dde2f6b4445fec85b\n")
    ///     .ok_or("not a version")?;
    ///
    /// assert_eq!(version.is_commit("0cc4ca7"), Some(true), "an abbreviation of it");
    /// assert_eq!(version.is_commit("bdf1045"), Some(false), "some other commit");
    /// assert_eq!(version.is_commit("0cc"), None, "too short to say anything");
    ///
    /// // a binary that prints no commit cannot be compared
    /// let bare = parse("komorebi 0.1.0").ok_or("not a version")?;
    /// assert_eq!(bare.is_commit("0cc4ca7"), None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn is_commit(&self, commit: &str) -> Option<bool> {
        let mine = self.commit.as_deref()?;
        let shorter = mine.len().min(commit.len());
        (shorter >= 7).then(|| mine[..shorter] == commit[..shorter])
    }
}

/// Reads the output of `komorebi --version`.
///
/// Blank lines are skipped and each line is trimmed. The first remaining line must be `komorebi`
/// followed by the version number. After it, the `branch:` and `commit_hash:` lines are read;
/// any other line is ignored. Returns `None` if the output is not that of komorebi: it is empty,
/// its first line does not start with `komorebi ` (so `komorebic 0.1.0` is not accepted), or the
/// version is empty.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_version::parse;
///
/// let version = parse("komorebi 0.1.0\nbranch:master\ncommit_hash:0cc4ca7f67d79da0d59d603dde2f6b4445fec85b\n")
///     .ok_or("not a version")?;
/// assert_eq!(version.version, "0.1.0");
/// assert_eq!(version.branch.as_deref(), Some("master"));
/// assert_eq!(version.commit.as_deref(), Some("0cc4ca7f67d79da0d59d603dde2f6b4445fec85b"));
/// assert_eq!(version.pid, None, "the text does not say which process printed it");
///
/// assert_eq!(parse("komorebic 0.1.0"), None);
/// assert_eq!(parse("error: unknown flag"), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn parse(text: &str) -> Option<KomorebiVersion> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let version = lines.next()?.strip_prefix("komorebi ")?.trim().to_string();
    if version.is_empty() {
        return None;
    }

    let mut found = KomorebiVersion {
        pid: None,
        version,
        branch: None,
        commit: None,
    };
    for line in lines {
        if let Some(branch) = line.strip_prefix("branch:") {
            found.branch = Some(branch.trim().to_string());
        } else if let Some(commit) = line.strip_prefix("commit_hash:") {
            found.commit = Some(commit.trim().to_string());
        }
    }

    Some(found)
}

/// Asks the binary at `exe` what it is, by running it with `--version`, and reads the answer with
/// [`parse`]. `pid` is stored in the result as the process the binary is running as.
///
/// The binary answers at once; one that has not exited within two seconds is killed and `None` is
/// returned. `None` is also returned if the binary cannot be run or its output is not that of
/// komorebi. The binary's standard input is closed and its standard error is discarded.
///
/// # Example
///
/// A binary that is not there is `None`; the case of a binary that prints a version is covered by
/// `a_binary_that_prints_a_version_is_queried` in this file's unit tests (on unix hosts, where it
/// writes a script to run):
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_version::query;
/// use std::path::Path;
///
/// assert_eq!(query(Path::new("/nonexistent/komorebi"), Some(42)), None);
/// ```
#[must_use]
pub fn query(exe: &Path, pid: Option<i32>) -> Option<KomorebiVersion> {
    let mut child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let output = child.wait_with_output().ok()?;
    let mut version = parse(&String::from_utf8_lossy(&output.stdout))?;
    version.pid = pid;
    Some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str =
        "komorebi 0.1.0\nbranch:master\ncommit_hash:0cc4ca7f67d79da0d59d603dde2f6b4445fec85b\n";

    #[test]
    fn the_real_output_is_read() {
        let version = parse(REAL).expect("version");
        assert_eq!(version.version, "0.1.0");
        assert_eq!(version.branch.as_deref(), Some("master"));
        assert_eq!(
            version.commit.as_deref(),
            Some("0cc4ca7f67d79da0d59d603dde2f6b4445fec85b")
        );
    }

    #[test]
    fn a_binary_that_says_less_is_still_a_version() {
        let version = parse("komorebi 0.1.0").expect("version");
        assert_eq!(version.commit, None);
        assert_eq!(version.is_commit("0cc4ca7"), None);
    }

    #[test]
    fn something_else_is_not_a_komorebi_version() {
        for text in ["", "komorebi", "komorebic 0.1.0", "error: unknown flag"] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn commits_compare_by_the_shorter_of_the_two() {
        let version = parse(REAL).expect("version");
        assert_eq!(version.is_commit("0cc4ca7"), Some(true));
        assert_eq!(
            version.is_commit("0cc4ca7f67d79da0d59d603dde2f6b4445fec85b"),
            Some(true)
        );
        assert_eq!(version.is_commit("bdf1045"), Some(false));
        // too short to say anything
        assert_eq!(version.is_commit("0cc"), None);
    }

    #[test]
    fn a_missing_binary_is_none() {
        assert_eq!(query(Path::new("/nonexistent/komorebi"), None), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_that_prints_a_version_is_queried() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("komorebi-version-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let script = dir.join("komorebi");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'komorebi 9.9.9\\nbranch:x\\ncommit_hash:abcdef1234567\\n'\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let version = query(&script, Some(42)).expect("version");
        assert_eq!(version.version, "9.9.9");
        assert_eq!(version.pid, Some(42));
        assert_eq!(version.commit.as_deref(), Some("abcdef1234567"));
    }
}
