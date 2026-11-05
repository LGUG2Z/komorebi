//! KDE global shortcuts as `.desktop` files.
//!
//! KDE has a place for "run this command when I press that": a `.desktop` file in a
//! `kglobalaccel` directory (`$XDG_DATA_HOME/kglobalaccel/`, or `share/kglobalaccel/` of a system
//! profile) whose `X-KDE-Shortcuts` key is the default shortcut of its launch action. KWin reads
//! the directory when it starts; the shortcuts then show up, and can be changed, in System
//! Settings, and they work wherever KWin sees the keys, with no komorebi process running. A file
//! added while KWin runs is not read until KWin is asked to register it, and a changed file not
//! until the next session.
//!
//! [`desktop_files`] writes one file per [`Shortcut`]; [`write()`] replaces the files it wrote
//! before with new ones, and [`diff`] says what a [`write()`] would change. Every generated file has
//! the key `X-Komorebi-Generated=true`, and [`write()`] removes no other file.
//!
//! ```rust
//! use komorebi_hotkeys::Format;
//! use komorebi_hotkeys::kde::{Shortcut, desktop_files};
//! use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
//!
//! let files = desktop_files(&[Shortcut {
//!     combo: Combo::new(Mods::ALT, key_to_qt(Format::Skhd, "h").unwrap()),
//!     command: "komorebic focus left".into(),
//!     comment: "from the skhdrc: alt - h : komorebic focus left".into(),
//! }]);
//! assert_eq!(files[0].name, "komorebi-focus-left.desktop");
//! assert!(files[0].content.contains("\nExec=komorebic focus left\n"));
//! assert!(files[0].content.contains("\nX-KDE-Shortcuts=Alt+H\n"));
//! ```

use crate::qt::Combo;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// The key every generated file has, so that a rewrite replaces them and nothing else.
const GENERATED: &str = "X-Komorebi-Generated";

/// A command on a key, to be written as a `.desktop` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// The keys.
    pub combo: Combo,
    /// The command to run.
    pub command: String,
    /// The file's `Comment`: where the binding came from, as its user wrote it.
    pub comment: String,
}

/// A `.desktop` file: its name in the directory, and its content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopFile {
    /// The file name, `komorebi-<the command>.desktop`.
    pub name: String,
    /// The content.
    pub content: String,
}

/// Returns the files for the shortcuts, with a different name each.
///
/// A file is named after its command, without a leading `komorebic `: `komorebic focus left` is
/// `komorebi-focus-left.desktop`. When two commands make the same name, the second file is
/// `-2`, the third `-3`, in the order of `shortcuts`.
///
/// The `Exec` line is the command when it has only letters, digits and ` _./:=,+@-%` in it, and
/// `sh -c "<the command>"` otherwise, quoted as the desktop entry specification requires. `%` is a
/// field code there, so it is written `%%`.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::kde::{Shortcut, desktop_files};
/// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
///
/// let shortcut = |key, command: &str| Shortcut {
///     combo: Combo::new(Mods::ALT, key_to_qt(Format::Skhd, key).unwrap()),
///     command: command.into(),
///     comment: String::new(),
/// };
/// let files = desktop_files(&[
///     shortcut("a", "komorebic retile"),
///     shortcut("b", "komorebic retile"),
///     shortcut("c", "echo $HOME"),
/// ]);
/// assert_eq!(files[0].name, "komorebi-retile.desktop");
/// assert_eq!(files[1].name, "komorebi-retile-2.desktop");
/// assert!(files[2].content.contains("\nExec=sh -c \"echo \\\\$HOME\"\n"));
/// ```
pub fn desktop_files(shortcuts: &[Shortcut]) -> Vec<DesktopFile> {
    let mut used: Vec<String> = Vec::new();
    shortcuts
        .iter()
        .map(|shortcut| {
            let base = format!("komorebi-{}", slug(&shortcut.command));
            let mut stem = base.clone();
            let mut n = 2;
            while used.contains(&stem) {
                stem = format!("{base}-{n}");
                n += 1;
            }
            used.push(stem.clone());
            desktop_file(shortcut, &stem)
        })
        .collect()
}

/// `komorebic focus left` -> `focus-left`; anything else is flattened the same way.
fn slug(command: &str) -> String {
    let words = command.strip_prefix("komorebic ").unwrap_or(command);
    let mut slug = String::new();
    for c in words.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').chars().take(48).collect()
}

/// A command as an `Exec=` value. The desktop entry spec quotes with `"` and escapes `"`, `` ` ``,
/// `$` and `\` inside them (and then again for the file format), and `%` is a field code.
fn exec_line(command: &str) -> String {
    let command = command.replace('%', "%%");
    let plain = command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " _./:=,+@-%".contains(c));
    if plain {
        return command;
    }

    let mut quoted = String::from("sh -c \"");
    for c in command.chars() {
        match c {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// One shortcut as a file.
fn desktop_file(shortcut: &Shortcut, file_stem: &str) -> DesktopFile {
    let words = shortcut
        .command
        .strip_prefix("komorebic ")
        .unwrap_or(&shortcut.command);

    let mut content = String::new();
    let _ = writeln!(content, "[Desktop Entry]");
    let _ = writeln!(content, "Type=Application");
    let _ = writeln!(content, "Name=komorebi: {words}");
    let _ = writeln!(content, "Comment={}", shortcut.comment);
    let _ = writeln!(content, "Exec={}", exec_line(&shortcut.command));
    let _ = writeln!(content, "Icon=preferences-system-windows");
    let _ = writeln!(content, "Terminal=false");
    let _ = writeln!(content, "StartupNotify=false");
    let _ = writeln!(content, "X-KDE-GlobalAccel-CommandShortcut=true");
    let _ = writeln!(content, "X-KDE-Shortcuts={}", shortcut.combo);
    let _ = writeln!(content, "{GENERATED}=true");

    DesktopFile {
        name: format!("{file_stem}.desktop"),
        content,
    }
}

/// What a [`write()`] did, or what it would do, by file name.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// Files that were not there before.
    pub added: Vec<String>,
    /// Files that were there, with different content.
    pub changed: Vec<String>,
    /// Files that were there and are not any more.
    pub removed: Vec<String>,
    /// Files that are exactly as they were.
    pub unchanged: Vec<String>,
}

/// Returns what [`write()`] would do to `dir`, without doing it.
///
/// A directory that does not exist has no files, so every file would be added.
///
/// # Errors
///
/// Fails when `dir` exists and cannot be listed. A file that cannot be read as text is not a
/// generated one, and is left alone.
pub fn diff(dir: &Path, files: &[DesktopFile]) -> std::io::Result<Written> {
    let before = if dir.is_dir() {
        generated(dir)?
    } else {
        BTreeMap::new()
    };
    Ok(compare(before, files))
}

/// Replaces the generated files in `dir` with `files`, leaving every other file alone, and
/// returns what changed.
///
/// A generated file is one whose name starts with `komorebi-`, ends in `.desktop`, and whose
/// content has `X-Komorebi-Generated=true`. `dir` is created if it does not exist.
///
/// # Errors
///
/// Fails when `dir` cannot be created or listed, a generated file cannot be removed, or a new one
/// cannot be written. What was removed or written before the failure stays so.
pub fn write(dir: &Path, files: &[DesktopFile]) -> std::io::Result<Written> {
    std::fs::create_dir_all(dir)?;
    let before = generated(dir)?;
    for name in before.keys() {
        std::fs::remove_file(dir.join(name))?;
    }
    for file in files {
        std::fs::write(dir.join(&file.name), &file.content)?;
    }
    Ok(compare(before, files))
}

/// The generated files of a directory, by name.
fn generated(dir: &Path) -> std::io::Result<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !(name.starts_with("komorebi-") && name.ends_with(".desktop")) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path)
            && content.contains(&format!("{GENERATED}=true"))
        {
            found.insert(name.to_string(), content);
        }
    }
    Ok(found)
}

fn compare(mut before: BTreeMap<String, String>, files: &[DesktopFile]) -> Written {
    let mut written = Written::default();
    for file in files {
        match before.remove(&file.name) {
            None => written.added.push(file.name.clone()),
            Some(old) if old == file.content => written.unchanged.push(file.name.clone()),
            Some(_) => written.changed.push(file.name.clone()),
        }
    }
    written.removed = before.into_keys().collect();
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Format;
    use crate::ParseOptions;
    use crate::plan::PlanMode;
    use crate::plan::PlanOptions;
    use crate::plan::plan;

    /// The files for the plain bindings of an skhdrc.
    fn files(skhdrc: &str) -> Vec<DesktopFile> {
        let parsed = crate::parse(Format::Skhd, skhdrc, &ParseOptions::default());
        let options = PlanOptions {
            mode: PlanMode::Auto,
        };
        desktop_files(&plan(&parsed.keymap, &options).static_files)
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("komorebi-hotkeys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_diff_says_what_a_write_would_do_and_does_not_do_it() {
        let dir = temp_dir("diff");
        let files = files("alt - h : komorebic focus left\nalt - l : komorebic focus right\n");

        // nothing there yet
        let before = diff(&dir, &files).unwrap();
        assert_eq!(before.added.len(), 2);
        assert!(!dir.exists(), "the diff wrote something");

        write(&dir, &files).unwrap();
        let after = diff(&dir, &files).unwrap();
        assert_eq!(after.unchanged.len(), 2);
        assert!(after.added.is_empty() && after.changed.is_empty() && after.removed.is_empty());

        // one binding gone from the skhdrc, one file of the user's own left alone
        std::fs::write(dir.join("mine.desktop"), "[Desktop Entry]\n").unwrap();
        let fewer = diff(&dir, &files[..1]).unwrap();
        assert_eq!(fewer.removed.len(), 1);
        assert!(dir.join("mine.desktop").exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn files_have_unique_names_and_the_default_shortcut() {
        let files = files("alt - a : komorebic retile\nalt - b : komorebic retile\n");
        assert_eq!(files[0].name, "komorebi-retile.desktop");
        assert_eq!(files[1].name, "komorebi-retile-2.desktop");
        assert!(files[0].content.contains("Exec=komorebic retile\n"));
        assert!(files[0].content.contains("X-KDE-Shortcuts=Alt+A\n"));
        assert!(files[1].content.contains("X-KDE-Shortcuts=Alt+B\n"));
        // kglobalacceld ignores desktop files marked NoDisplay
        assert!(!files[0].content.contains("NoDisplay"));
    }

    #[test]
    fn a_file_is_written_as_kglobalacceld_reads_it() {
        let files = files("alt + shift - 0x21      : komorebic cycle-focus previous # 0x21 is [\n");
        assert_eq!(
            files[0].content,
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=komorebi: cycle-focus previous\n\
             Comment=from the skhdrc: alt + shift - 0x21      : komorebic cycle-focus previous # 0x21 is [\n\
             Exec=komorebic cycle-focus previous\n\
             Icon=preferences-system-windows\n\
             Terminal=false\n\
             StartupNotify=false\n\
             X-KDE-GlobalAccel-CommandShortcut=true\n\
             X-KDE-Shortcuts=Alt+{\n\
             X-Komorebi-Generated=true\n"
        );
    }

    #[test]
    fn exec_lines_are_quoted_for_the_desktop_entry_spec() {
        assert_eq!(exec_line("komorebic focus left"), "komorebic focus left");
        assert_eq!(
            exec_line("komorebic focus-workspace 0"),
            "komorebic focus-workspace 0"
        );
        assert_eq!(exec_line("notify-send 100%"), "notify-send 100%%");
        assert_eq!(exec_line("echo $HOME"), "sh -c \"echo \\\\$HOME\"");
        assert_eq!(exec_line("a | b"), "sh -c \"a | b\"");
    }

    #[test]
    fn slugs_are_short_and_plain() {
        assert_eq!(slug("komorebic focus left"), "focus-left");
        assert_eq!(slug("notify-send 'a  b'"), "notify-send-a-b");
        assert_eq!(slug(&"x ".repeat(40)).len(), 48);
    }

    #[test]
    fn rewriting_replaces_our_files_and_nothing_else() {
        let dir = temp_dir("rewrite");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("someone-elses.desktop"),
            "[Desktop Entry]\nName=x\n",
        )
        .unwrap();
        // looks like ours by name, but was not generated
        std::fs::write(
            dir.join("komorebi-by-hand.desktop"),
            "[Desktop Entry]\nName=mine\n",
        )
        .unwrap();

        let first = files("alt - a : komorebic retile\nalt - b : komorebic stop\n");
        let first_run = write(&dir, &first).unwrap();
        assert_eq!(first_run.added.len(), 2);
        assert!(first_run.removed.is_empty());

        // one binding goes, one stays, and one is rebound to another key
        let second = files("alt - c : komorebic retile\n");
        let second_run = write(&dir, &second).unwrap();
        assert_eq!(second_run.removed, ["komorebi-stop.desktop"]);
        assert_eq!(second_run.changed, ["komorebi-retile.desktop"]);

        let third_run = write(&dir, &second).unwrap();
        assert_eq!(third_run.unchanged, ["komorebi-retile.desktop"]);
        assert!(third_run.added.is_empty() && third_run.changed.is_empty());

        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "komorebi-by-hand.desktop",
                "komorebi-retile.desktop",
                "someone-elses.desktop"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
