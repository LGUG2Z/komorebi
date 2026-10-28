//! KWin's configuration file, `kwinrc`, for the few settings komorebi changes: per-output virtual
//! desktops, the focus policy, and the settings of the border and animation effects.
//!
//! These are the user's settings, changed on their behalf, so [`Kwinrc::set`] records the value of
//! every key the first time komorebi changes it, in a backup file, and [`Kwinrc::restore`] puts the
//! values back when komorebi exits. The backup outlives a crash: the next run finds it, keeps the
//! values it recorded (not the ones the crashed run wrote), and restores them at the end of that
//! run. The KWin backend keeps the backup in [`DATA_DIR`](struct@crate::DATA_DIR), which is under
//! `$XDG_RUNTIME_DIR` when there is one: such a directory is emptied at logout, so a backup a crash
//! left behind lasts until the session ends.
//!
//! KDE's tools (`kreadconfig6`, `kwriteconfig6`) are not used. The daemon runs as a user service
//! whose PATH need not have them: in a live Plasma 6.8.1 session, at time of writing (2026-10-27),
//! `kwriteconfig6` was not on that PATH, and per-output desktops stayed off. The file is plain ini,
//! which KConfig reads back, and a setting that silently was not changed is worse than one changed
//! by editing a file.
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | A key komorebi changed gets back the value it had before komorebi first changed it, and a key komorebi added is removed | [`Kwinrc::set`] records only the first value of a key; [`Kwinrc::restore`] | `kwin::kwinrc::tests::what_komorebi_changed_is_put_back_exactly_and_what_it_added_is_removed` |
//! | A crash loses none of the recorded values | the backup file, read by the next run | `kwin::kwinrc::tests::a_backup_left_by_a_crash_keeps_the_values_from_before_the_first_run` |
//! | Only the line of the key changes; the rest of the file is left as it was | [`ini_set`], [`ini_unset`] | `kwin::kwinrc::tests::a_key_is_set_in_place_added_to_its_group_or_given_a_group`, `kwin::kwinrc::tests::a_key_is_removed_from_its_own_group_only` |
//! | One group can be put back while the others stay changed | [`Kwinrc::restore_group`] | `kwin::kwinrc::tests::one_group_can_be_put_back_while_the_others_stay_changed` |
//! | Some keys of a group can be put back while the others stay changed | [`Kwinrc::restore_keys`] | `kwin::kwinrc::tests::one_key_can_be_put_back_while_the_others_in_its_group_stay_changed` |
//! | KWin never reads a half-written file | [`Kwinrc`] writes a file beside `kwinrc` and renames it over it | no test |

#![deny(missing_docs)]

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

/// The value of each key before komorebi changed it, by group and key: `None` for a key that was
/// not in the user's file.
type Originals = BTreeMap<String, BTreeMap<String, Option<String>>>;

/// A `kwinrc` file that komorebi changes, and the backup of the values it changed.
pub struct Kwinrc {
    /// The `kwinrc` that is read and written.
    path: PathBuf,
    /// The JSON file the original values are kept in.
    backup: PathBuf,
}

/// The contents of the backup file.
#[derive(Serialize, Deserialize, Default)]
struct Backup {
    /// The value of every key komorebi changed, before it first changed it.
    originals: Originals,
}

impl Kwinrc {
    /// Returns a `kwinrc` at `path`, with its backup at `backup`. Neither file has to exist.
    pub fn new(path: PathBuf, backup: PathBuf) -> Self {
        Self { path, backup }
    }

    /// Returns the user's `kwinrc`, with the backup as `kwinrc-backup.json` in `data_dir`.
    ///
    /// The user's `kwinrc` is in `$XDG_CONFIG_HOME`, or in `~/.config` when that is not set.
    /// Returns `None` when neither can be found.
    pub fn user(data_dir: &std::path::Path) -> Option<Self> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;

        Some(Self::new(
            config.join("kwinrc"),
            data_dir.join("kwinrc-backup.json"),
        ))
    }

    /// Returns the value of `key` in `[group]`, read the way KConfig cascades: from the user's file
    /// if it has the key, else from the first `kwinrc` in `$XDG_CONFIG_DIRS` (`/etc/xdg` when that
    /// is not set) that has it.
    pub fn get(&self, group: &str, key: &str) -> Option<String> {
        let system = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_string());

        std::iter::once(self.path.clone())
            .chain(
                system
                    .split(':')
                    .map(|dir| PathBuf::from(dir).join("kwinrc")),
            )
            .filter_map(|file| std::fs::read_to_string(file).ok())
            .find_map(|text| ini_value(&text, group, key))
    }

    /// Returns true if and only if the setting is `true`, as [`Kwinrc::get`] reads it.
    pub fn is_on(&self, group: &str, key: &str) -> bool {
        self.get(group, key).as_deref() == Some("true")
    }

    /// Sets keys in a group, recording the value of each in the backup the first time komorebi
    /// changes it.
    ///
    /// A key set a second time keeps the value recorded the first time, in this run or in one that
    /// crashed, so [`Kwinrc::restore`] puts back what the user had.
    ///
    /// # Errors
    ///
    /// Returns an error if the backup or `kwinrc` cannot be written. The backup is written first:
    /// when it cannot be, `kwinrc` is left as it was.
    pub fn set(&self, group: &str, pairs: &[(&str, String)]) -> io::Result<()> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();

        let mut backup = self.read_backup();
        let mut changed = false;
        for (key, _) in pairs {
            let originals = backup.originals.entry(group.to_string()).or_default();
            if !originals.contains_key(*key) {
                originals.insert((*key).to_string(), ini_value(&text, group, key));
                changed = true;
            }
        }
        if changed {
            self.write_backup(&backup)?;
        }

        let mut updated = text;
        for (key, value) in pairs {
            updated = ini_set(&updated, group, key, value);
        }
        self.write_file(&updated)
    }

    /// Puts back everything [`Kwinrc::set`] changed, removes the backup, and returns true if and
    /// only if there was anything to put back.
    ///
    /// # Errors
    ///
    /// Returns an error if `kwinrc` cannot be written, or if the backup cannot be removed after
    /// `kwinrc` was written.
    pub fn restore(&self) -> io::Result<bool> {
        let backup = self.read_backup();
        if backup.originals.is_empty() {
            let _ = std::fs::remove_file(&self.backup);
            return Ok(false);
        }

        let mut text = std::fs::read_to_string(&self.path).unwrap_or_default();
        for (group, keys) in &backup.originals {
            for (key, original) in keys {
                text = match original {
                    Some(value) => ini_set(&text, group, key, value),
                    None => ini_unset(&text, group, key),
                };
            }
        }

        self.write_file(&text)?;
        std::fs::remove_file(&self.backup)?;
        Ok(true)
    }

    /// Puts back what [`Kwinrc::set`] changed in one group, and returns true if and only if it
    /// changed any key of the group.
    ///
    /// The other groups stay as komorebi changed them, and so does the backup of their values.
    ///
    /// # Errors
    ///
    /// Returns an error if `kwinrc` or the backup cannot be written.
    pub fn restore_group(&self, group: &str) -> io::Result<bool> {
        let mut backup = self.read_backup();
        let Some(keys) = backup.originals.remove(group) else {
            return Ok(false);
        };

        let mut text = std::fs::read_to_string(&self.path).unwrap_or_default();
        for (key, original) in &keys {
            text = match original {
                Some(value) => ini_set(&text, group, key, value),
                None => ini_unset(&text, group, key),
            };
        }
        self.write_file(&text)?;

        if backup.originals.is_empty() {
            let _ = std::fs::remove_file(&self.backup);
        } else {
            self.write_backup(&backup)?;
        }
        Ok(true)
    }

    /// Puts back these keys of one group, leaving the rest of what [`Kwinrc::set`] changed as it
    /// is, and returns true if and only if [`Kwinrc::set`] changed any of them.
    ///
    /// `[Windows]` has both the focus policy and per-output desktops, which are turned off at
    /// different times.
    ///
    /// # Errors
    ///
    /// Returns an error if `kwinrc` or the backup cannot be written.
    pub fn restore_keys(&self, group: &str, wanted: &[&str]) -> io::Result<bool> {
        let mut backup = self.read_backup();
        let Some(keys) = backup.originals.get_mut(group) else {
            return Ok(false);
        };

        let taken: Vec<(String, Option<String>)> = wanted
            .iter()
            .filter_map(|key| {
                keys.remove(*key)
                    .map(|original| ((*key).to_string(), original))
            })
            .collect();
        if taken.is_empty() {
            return Ok(false);
        }
        if keys.is_empty() {
            backup.originals.remove(group);
        }

        let mut text = std::fs::read_to_string(&self.path).unwrap_or_default();
        for (key, original) in &taken {
            text = match original {
                Some(value) => ini_set(&text, group, key, value),
                None => ini_unset(&text, group, key),
            };
        }
        self.write_file(&text)?;

        if backup.originals.is_empty() {
            let _ = std::fs::remove_file(&self.backup);
        } else {
            self.write_backup(&backup)?;
        }
        Ok(true)
    }

    /// Returns the backup, or an empty one when there is no backup file or it cannot be read.
    fn read_backup(&self) -> Backup {
        std::fs::read_to_string(&self.backup)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the backup, creating its directory if needed.
    fn write_backup(&self, backup: &Backup) -> io::Result<()> {
        if let Some(dir) = self.backup.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(
            &self.backup,
            serde_json::to_string_pretty(backup).map_err(io::Error::other)?,
        )
    }

    /// Writes `kwinrc`: the text goes into a file beside it, which is then renamed over it, so that
    /// KWin never reads half of it.
    fn write_file(&self, text: &str) -> io::Result<()> {
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_default();
        std::fs::create_dir_all(&dir)?;
        let temporary = dir.join(".kwinrc.komorebi");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &self.path)
    }
}

/// Returns the value of `key` in `[group]` of the ini text, trimmed, or `None` when the group does
/// not have the key.
///
/// Covered by `kwin::kwinrc::tests::a_key_is_read_from_its_own_group_only`.
pub fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_group = name == group;
        } else if in_group
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Returns the ini text with `key=value` in `[group]`: the line of the key is replaced if there is
/// one, otherwise the line is added at the end of the group, and the group is added at the end of
/// the text when there is none.
///
/// Every other line is left as it was, and the text returned ends with a newline. Covered by
/// `kwin::kwinrc::tests::a_key_is_set_in_place_added_to_its_group_or_given_a_group`.
pub fn ini_set(text: &str, group: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_group = false;
    let mut done = false;

    let flush = |out: &mut Vec<String>, done: &mut bool, in_group: bool| {
        // leaving the group without having found the key: it goes at the end of the group
        if in_group && !*done {
            let mut at = out.len();
            while at > 0 && out[at - 1].trim().is_empty() {
                at -= 1;
            }
            out.insert(at, format!("{key}={value}"));
            *done = true;
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&mut out, &mut done, in_group);
            in_group = name == group;
            out.push(line.to_string());
        } else if in_group
            && !done
            && let Some((k, _)) = trimmed.split_once('=')
            && k.trim() == key
        {
            out.push(format!("{key}={value}"));
            done = true;
        } else {
            out.push(line.to_string());
        }
    }
    flush(&mut out, &mut done, in_group);

    if !done {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(format!("[{group}]"));
        out.push(format!("{key}={value}"));
    }

    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Returns the ini text without `key` in `[group]`.
///
/// A group left empty stays in place, which KConfig accepts. Every other line is left as it was.
/// Covered by `kwin::kwinrc::tests::a_key_is_removed_from_its_own_group_only`.
pub fn ini_unset(text: &str, group: &str, key: &str) -> String {
    let mut in_group = false;
    let mut out: Vec<&str> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_group = name == group;
        } else if in_group
            && let Some((k, _)) = trimmed.split_once('=')
            && k.trim() == key
        {
            continue;
        }
        out.push(line);
    }

    let mut joined = out.join("\n");
    if text.ends_with('\n') || !joined.is_empty() {
        joined.push('\n');
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(format!(
            "/tmp/komorebi-kwinrc-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_key_is_read_from_its_own_group_only() {
        let rc = "[General]\nK=true\n\n[Windows]\nFocusPolicy=1\nK = false \n\n[Other]\nK=true\n";
        assert_eq!(ini_value(rc, "Windows", "K").as_deref(), Some("false"));
        assert_eq!(ini_value(rc, "Windows", "Missing"), None);
        assert_eq!(ini_value(rc, "Nowhere", "FocusPolicy"), None);
    }

    #[test]
    fn a_key_is_set_in_place_added_to_its_group_or_given_a_group() {
        assert_eq!(
            ini_set(
                "[A]\nx=1\n\n[Windows]\nFocusPolicy=1\nK=old\n\n[B]\ny=2\n",
                "Windows",
                "K",
                "new"
            ),
            "[A]\nx=1\n\n[Windows]\nFocusPolicy=1\nK=new\n\n[B]\ny=2\n"
        );
        assert_eq!(
            ini_set(
                "[Windows]\nFocusPolicy=1\n\n[B]\ny=2\n",
                "Windows",
                "K",
                "v"
            ),
            "[Windows]\nFocusPolicy=1\nK=v\n\n[B]\ny=2\n"
        );
        assert_eq!(
            ini_set("[A]\nx=1\n\n[Windows]\nFocusPolicy=1", "Windows", "K", "v"),
            "[A]\nx=1\n\n[Windows]\nFocusPolicy=1\nK=v\n"
        );
        assert_eq!(
            ini_set("[A]\nx=1\n", "Windows", "K", "v"),
            "[A]\nx=1\n\n[Windows]\nK=v\n"
        );
        assert_eq!(ini_set("", "Windows", "K", "v"), "[Windows]\nK=v\n");
        assert_eq!(
            ini_set("[Other]\nK=1\n", "Windows", "K", "v"),
            "[Other]\nK=1\n\n[Windows]\nK=v\n"
        );
    }

    #[test]
    fn a_key_is_removed_from_its_own_group_only() {
        assert_eq!(
            ini_unset("[A]\nK=1\n[Windows]\nK=2\nJ=3\n", "Windows", "K"),
            "[A]\nK=1\n[Windows]\nJ=3\n"
        );
    }

    #[test]
    fn what_komorebi_changed_is_put_back_exactly_and_what_it_added_is_removed() {
        let d = dir("backup");
        let rc = Kwinrc::new(d.join("kwinrc"), d.join("backup.json"));
        let original = "[Windows]\nFocusPolicy=1\nShade=true\n\n[Round-Corners]\nSize=8\n";
        std::fs::write(d.join("kwinrc"), original).unwrap();

        rc.set(
            "Round-Corners",
            &[
                ("Size", "20".into()),
                ("OuterOutlineThickness", "10".into()),
            ],
        )
        .unwrap();
        rc.set("Windows", &[("PerOutputVirtualDesktops", "true".into())])
            .unwrap();
        // a second change to a key already changed must not overwrite what it was before komorebi
        rc.set("Round-Corners", &[("Size", "30".into())]).unwrap();

        let changed = std::fs::read_to_string(d.join("kwinrc")).unwrap();
        assert_eq!(
            ini_value(&changed, "Round-Corners", "Size").as_deref(),
            Some("30")
        );
        assert!(rc.is_on("Windows", "PerOutputVirtualDesktops"));

        assert!(rc.restore().unwrap());
        let back = std::fs::read_to_string(d.join("kwinrc")).unwrap();
        assert_eq!(
            ini_value(&back, "Round-Corners", "Size").as_deref(),
            Some("8")
        );
        assert_eq!(
            ini_value(&back, "Round-Corners", "OuterOutlineThickness"),
            None
        );
        assert_eq!(
            ini_value(&back, "Windows", "PerOutputVirtualDesktops"),
            None
        );
        assert_eq!(
            ini_value(&back, "Windows", "FocusPolicy").as_deref(),
            Some("1")
        );
        assert!(!d.join("backup.json").exists());

        // nothing left to restore
        assert!(!rc.restore().unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_backup_left_by_a_crash_keeps_the_values_from_before_the_first_run() {
        let d = dir("crash");
        let first = Kwinrc::new(d.join("kwinrc"), d.join("backup.json"));
        std::fs::write(d.join("kwinrc"), "[Windows]\nK=orig\n").unwrap();
        first.set("Windows", &[("K", "komorebi".into())]).unwrap();

        // the daemon dies; the next one starts with the changed file and the backup
        let second = Kwinrc::new(d.join("kwinrc"), d.join("backup.json"));
        second
            .set("Windows", &[("K", "komorebi again".into())])
            .unwrap();
        second.restore().unwrap();

        let back = std::fs::read_to_string(d.join("kwinrc")).unwrap();
        assert_eq!(ini_value(&back, "Windows", "K").as_deref(), Some("orig"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_group_can_be_put_back_while_the_others_stay_changed() {
        let d = dir("group");
        let rc = Kwinrc::new(d.join("kwinrc"), d.join("backup.json"));
        std::fs::write(d.join("kwinrc"), "[Round-Corners]\nSize=8\n").unwrap();

        rc.set("Round-Corners", &[("Size", "20".into())]).unwrap();
        rc.set("Windows", &[("PerOutputVirtualDesktops", "true".into())])
            .unwrap();

        assert!(rc.restore_group("Round-Corners").unwrap());
        let text = std::fs::read_to_string(d.join("kwinrc")).unwrap();
        assert_eq!(
            ini_value(&text, "Round-Corners", "Size").as_deref(),
            Some("8")
        );
        assert!(rc.is_on("Windows", "PerOutputVirtualDesktops"));

        // nothing of it left to put back, and the other group still can be
        assert!(!rc.restore_group("Round-Corners").unwrap());
        assert!(d.join("backup.json").exists());
        assert!(rc.restore().unwrap());
        assert!(!d.join("backup.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_key_can_be_put_back_while_the_others_in_its_group_stay_changed() {
        let d = dir("keys");
        let rc = Kwinrc::new(d.join("kwinrc"), d.join("backup.json"));
        std::fs::write(d.join("kwinrc"), "[Windows]\nFocusPolicy=0\n").unwrap();

        rc.set("Windows", &[("FocusPolicy", "1".into())]).unwrap();
        rc.set("Windows", &[("PerOutputVirtualDesktops", "true".into())])
            .unwrap();

        assert!(rc.restore_keys("Windows", &["FocusPolicy"]).unwrap());
        let text = std::fs::read_to_string(d.join("kwinrc")).unwrap();
        assert_eq!(
            ini_value(&text, "Windows", "FocusPolicy").as_deref(),
            Some("0")
        );
        assert!(rc.is_on("Windows", "PerOutputVirtualDesktops"));

        // nothing of it left to put back, and the other key still can be
        assert!(!rc.restore_keys("Windows", &["FocusPolicy"]).unwrap());
        assert!(rc.restore().unwrap());
        let text = std::fs::read_to_string(d.join("kwinrc")).unwrap();
        assert_eq!(
            ini_value(&text, "Windows", "PerOutputVirtualDesktops"),
            None
        );
        assert!(!d.join("backup.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
