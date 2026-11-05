//! Showing a folder, asking for one, and naming the OS.
//!
//! The recorder window offers to show a finished session in the file manager, the viewer asks for
//! a session folder, and the manifest records the OS version. On macOS these are `open`,
//! `osascript` and `sw_vers`; the same items exist on Windows.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// What the OS calls its file manager, for button labels: `Finder`.
pub const FILE_MANAGER: &str = "Finder";

/// Returns the OS version as `sw_vers -productVersion` prints it, e.g. `26.0`, or `None` if that
/// cannot be run or prints nothing. On Windows it is `major.minor.build`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::shell::{FILE_MANAGER, os_version};
///
/// assert_eq!(FILE_MANAGER, "Finder");
/// if let Some(version) = os_version() {
///     assert!(version.chars().all(|c| c.is_ascii_digit() || c == '.'), "{version}");
/// }
/// ```
#[must_use]
pub fn os_version() -> Option<String> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}

/// Shows `path` in the file manager.
///
/// This runs `open path` and does not wait for it, and a failure to start it is not reported. A
/// folder opens in Finder. A file opens in its default application, not selected in Finder (`open
/// -R` would select it); the recorder only passes folders. On Windows a file is selected in its
/// folder.
///
/// This starts another program, so it has no doctest.
pub fn reveal(path: &Path) {
    let _ = Command::new("open").arg(path).spawn();
}

/// Asks the user for a folder with the system's own dialog, and blocks until they answer.
///
/// Returns the folder they chose, or `None` if they canceled, the dialog could not be shown, or
/// the path came back empty. `prompt` is the dialog's prompt text. It is not escaped: it is placed
/// inside an `AppleScript` string, so it should not contain `"` or `\`.
///
/// This shows a dialog, so it has no doctest.
#[must_use]
pub fn pick_folder(prompt: &str) -> Option<PathBuf> {
    let output = Command::new("osascript")
        .args([
            "-e",
            &format!("POSIX path of (choose folder with prompt \"{prompt}\")"),
        ])
        .output()
        .ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}
