//! Keeping the recorder's own window out of the video: a KWin window rule.
//!
//! KWin has a window property, "exclude from capture", that leaves a window out of every screencast
//! (and out of the windows that are transient for it). A program that is not the shell cannot set
//! it for itself (the protocol that does is privileged), but a *window rule* can: the one written
//! here matches the window class of the recorder ([`APP_ID`]) and forces the property. It is the
//! rule "Exclude from screen capture" of System Settings, written to `kwinrulesrc` and applied by
//! asking KWin to reconfigure.
//!
//! The rule is written once, is found again by its group name, and is never removed: it concerns
//! no window but the recorder's. The text is edited as text, so that what else is in the file is
//! left alone.

use crate::komorebi_ignore::APP_ID;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use std::path::PathBuf;

/// The group of the rule in `kwinrulesrc`.
const GROUP: &str = "komorebi-recorder-exclude-from-capture";

/// The group of the rule, as the text of the group.
fn group_text() -> String {
    format!(
        "[{GROUP}]\n\
         Description=komorebi-recorder is left out of screen captures\n\
         excludefromcapture=true\n\
         excludefromcapturerule=2\n\
         wmclass={APP_ID}\n\
         wmclasscomplete=false\n\
         wmclassmatch=1\n"
    )
}

/// Returns `rules` with the rule added, or `None` when it is there already.
///
/// A rule exists for KWin once its group is named in the list of `[General]`: `Order=` in KWin 6.8
/// (which rewrites a file in that form), or `rules=` with `count=` in the form before it. The list
/// the file has is the one that is added to, and a file with neither gets `Order=`.
#[must_use]
pub fn with_rule(rules: &str) -> Option<String> {
    if rules
        .lines()
        .any(|line| line.trim() == format!("[{GROUP}]"))
    {
        return None;
    }
    let legacy = rules.lines().any(|line| line.trim().starts_with("rules="));
    let (key, count) = if legacy {
        ("rules=", true)
    } else {
        ("Order=", false)
    };

    let mut kept = Vec::new();
    let mut general = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut in_general = false;
    for line in rules.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_general = trimmed == "[General]";
            if in_general {
                continue;
            }
        }
        if !in_general {
            kept.push(line.to_string());
        } else if let Some(list) = trimmed.strip_prefix(key) {
            names = list
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
        } else if !(count && trimmed.starts_with("count=")) {
            general.push(line.to_string());
        }
    }
    names.push(GROUP.to_string());
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&group_text());
    out.push_str("\n[General]\n");
    for line in general {
        out.push_str(&line);
        out.push('\n');
    }
    if count {
        out.push_str(&format!("count={}\n", names.len()));
    }
    out.push_str(&format!("{key}{}\n", names.join(",")));
    Some(out)
}

fn rules_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("kwinrulesrc"))
}

/// Returns `true` if and only if the rule is in the user's `kwinrulesrc`.
#[must_use]
pub fn installed() -> bool {
    rules_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|text| with_rule(&text).is_none())
}

/// Asks KWin to read its configuration again.
fn reconfigure() -> eyre::Result<()> {
    zbus::blocking::Connection::session()?.call_method(
        Some("org.kde.KWin"),
        "/KWin",
        Some("org.kde.KWin"),
        "reconfigure",
        &(),
    )?;
    Ok(())
}

/// Makes sure the rule is written and KWin has it.
///
/// # Errors
///
/// Returns an error when the file cannot be read or written, or KWin cannot be asked.
pub fn ensure() -> eyre::Result<()> {
    let path = rules_path().ok_or_else(|| eyre::eyre!("there is no configuration directory"))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).wrap_err_with(|| format!("reading {}", path.display())),
    };
    let Some(text) = with_rule(&text) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text).wrap_err_with(|| format!("writing {}", path.display()))?;
    reconfigure().wrap_err("asking KWin to read its window rules")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_is_added_to_an_empty_file() {
        let text = with_rule("").expect("added");
        assert!(text.contains("wmclass=komorebi-recorder"));
        assert!(text.contains("excludefromcapturerule=2"));
        assert!(text.ends_with("[General]\nOrder=komorebi-recorder-exclude-from-capture\n"));
    }

    #[test]
    fn a_file_in_the_form_of_kwin_6_8_is_added_to_by_order() {
        let before = "[1]\nDescription=mine\nwmclass=foo\n\n[General]\nOrder=1\n";
        let after = with_rule(before).expect("added");
        assert!(after.ends_with("Order=1,komorebi-recorder-exclude-from-capture\n"));
        assert!(!after.contains("count="));
        assert_eq!(with_rule(&after), None);
    }

    #[test]
    fn a_file_in_the_older_form_keeps_its_count_and_rules() {
        let before = "[1]\nDescription=mine\nwmclass=foo\n\n[General]\ncount=1\nrules=1\n";
        let after = with_rule(before).expect("added");
        assert!(after.contains("count=2\nrules=1,komorebi-recorder-exclude-from-capture\n"));
        assert!(!after.contains("Order="));
    }

    #[test]
    fn it_is_added_once_and_the_other_rules_are_kept() {
        let before = "[1]\nDescription=mine\nwmclass=foo\n\n[General]\nOrder=1\n";
        let after = with_rule(before).expect("added");
        assert!(after.contains("[1]\nDescription=mine\nwmclass=foo\n"));
        assert!(after.contains("Order=1,komorebi-recorder-exclude-from-capture\n"));
        assert_eq!(after.matches("[General]").count(), 1);
        assert_eq!(with_rule(&after), None);
    }
}
