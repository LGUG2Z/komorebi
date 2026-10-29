//! Showing a folder, asking for one, and naming the OS, the way a Plasma desktop does it.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// What this host calls its file manager, for button labels.
pub const FILE_MANAGER: &str = "file manager";

/// Returns the distribution's `PRETTY_NAME` or, failing that, its `VERSION_ID`, from
/// `/etc/os-release`.
#[must_use]
pub fn os_version() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .ok()?;
    os_release_field(&text, "PRETTY_NAME").or_else(|| os_release_field(&text, "VERSION_ID"))
}

fn os_release_field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix('=')?;
        let value = value.trim().trim_matches('"').trim_matches('\'');
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Asks `xdg-open` to show `path`, which Plasma hands to Dolphin.
pub fn reveal(path: &Path) {
    let _ = Command::new("xdg-open").arg(path).spawn();
}

/// Asks for a folder with `kdialog`, or `zenity` where there is no `kdialog`.
///
/// Returns `None` if neither is installed or the dialog was cancelled.
#[must_use]
pub fn pick_folder(prompt: &str) -> Option<PathBuf> {
    let asked = Command::new("kdialog")
        .args(["--title", prompt, "--getexistingdirectory"])
        .output()
        .or_else(|_| {
            Command::new("zenity")
                .args(["--file-selection", "--directory", "--title", prompt])
                .output()
        })
        .ok()?;
    if !asked.status.success() {
        return None;
    }
    let text = String::from_utf8(asked.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pretty_name_is_read_without_its_quotes() {
        let text = "NAME=\"NixOS\"\nPRETTY_NAME=\"NixOS 25.11 (Xantusia)\"\nVERSION_ID=\"25.11\"\n";
        assert_eq!(
            os_release_field(text, "PRETTY_NAME").as_deref(),
            Some("NixOS 25.11 (Xantusia)")
        );
        assert_eq!(
            os_release_field(text, "VERSION_ID").as_deref(),
            Some("25.11")
        );
        assert_eq!(os_release_field(text, "MISSING"), None);
    }
}
